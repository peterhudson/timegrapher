# Desktop app

`timegrapher-app` is the live timegrapher screen: put the watch on the
microphone and it shows the paper-strip trace with rate, amplitude and beat
error, the way a bench timegrapher does. It can also replay a recording as
if it were live, or analyse a whole recording at once and let you look
back through it.

```sh
cargo run --release -p timegrapher-app             # live from a microphone
cargo run --release -p timegrapher-app -- rec.flac  # replay a recording
```

On Linux, building it needs the ALSA headers (`sudo apt install libasound2-dev`).

## The screen

Along the top, the toolbar: the sidebar button, which hides the sidebar
to give the panes the whole window and brings it back (Ctrl+B, or Cmd+B on
a Mac), **Microphone** or **Recording**, the
microphone menu (or the file to open), the input's level meter and the
buttons for the session (see Sessions below; **Replay**, **Stop** and
**Analyse All** for a recording). On the left, the settings in cards:
**Watch**, **Microphone** (only for the microphone), then one card for each
part of the window (**Readings**, **Paper Strip**, **Tick Tock Profile**,
**Charts**, **Distributions**), each with its switch to show or hide it and
its own options, and **Window** for the appearance and the pane layout.
Clicking a card's name folds the card away to its caption, and clicking it
again opens it. A choice between two options, such as Stacked and Beside,
switches to the other option wherever it is clicked, so it can be flipped
back and forth without moving the pointer. Titles and labels are in Title
Case. In the rest, the
three readings in cards and below them the panes. Along the bottom, the
status line. Every card and reading has a **?** beside its name that opens
a longer explanation in place (click it again, or anywhere else, to close
it), and every setting gives a short hint when the pointer rests on it. Before anything is
open, the main area says how to begin, with buttons to start listening or
open a recording; a problem shows in a red banner above the readings until
it is dismissed or the next start clears it. The app follows the
desktop's light or dark setting (Appearance, under Window, overrides it),
and uses the Inter typeface with figures of even width, so readings don't
shift as they change.

**Readings.** Rate (seconds per day gained, +, or lost, −), amplitude (degrees) and beat error (ms), each a
fit over the last few seconds of beats (10 s by default; type any time
from 1 s to an hour, or pick one of Witschi's 2 to 60 s; the setting is
in the Readings card). Early in a session, or after a pause or a gap of
more than 2 s with no beats, there are fewer seconds of beats than asked
for: the readings use what there is and say so ("190 beats in 24 s of
180 s"); they never fit across a pause or gap, since the watch may have
been moved or have drifted meanwhile. The big amplitude is the average of the two
swings; under it are the amplitude from the ticks (blue on the strip and
charts) and from the tocks (orange). The sound can't tell which beat is
which pallet, so the first beat heard is called the tick. A big gap
between tick and tock usually means one side's sounds were misread rather than a fault in the
watch;
which side is the entry pallet can't be told from the sound, so the sign
of the beat error follows which beat came first, as on tg. The big beat
error is timed from the unlock, as tg and commercial timegraphers measure
it; beside it, smaller, is the beat error timed from the beat as a whole,
nearer the drop, which is the gap between the two lines on the strip.
Beside the rate, when it is at least 0.05 s/d, is its ± figure: the
standard error of the fitted slope, roughly how far the rate could be off
from the scatter of the beats alone; it shrinks with a longer averaging
time. Under the rate: how many beats the readings used and
the jitter: how far single beats land from the steady line the rate and
beat error are fitted to (a robust standard deviation, in microseconds),
which is the spread of the dots across the strip. Lower is steadier; it
rises with noise or a muffled sound as well as with a watch that runs
unevenly, so compare it on the same stand and microphone. The **?** on
each reading explains it at length.

**Panes.** Below the readings are five panes: the paper strip, the tick tock
profile, and charts of rate, amplitude and beat error over the session. Drag a pane by its tab
to put it beside, above or below another, or onto another's tab to stack
them; drag the gaps between panes to resize them. The × on a tab hides
that pane, and the switch on each card in the sidebar hides or shows its pane
or the readings across the top; a hidden pane comes back where it was,
and changing the strip's direction keeps the choice. **Reset Panes**
(under Window) shows them all in their starting places. Periodicity views will join
them.

**Tick Tock Profile.** The typical sound of the ticks (blue) and of
the tocks (orange) over the averaging time (Average Over, up to 60 s, so
it describes the same beats as the readings; a longer time gives a
steadier shape, and after lengthening it the beat count fills in as beats
arrive), from the drop's side of the beat back
through the unlock, with a shaded band where the middle 80% of beats fall
and the marks the engine read: unlock (green) and drop (red), from which
amplitude and the unlock-based beat error come, the drop's peak (purple),
the three sounds where the engine tells them apart (dashed gold: 1 unlock,
2 impulse, 3 drop) and the noise floor. Each line is named on the plot.
The solid edges and the dashed sounds are two different measurements of
the same averaged sound, not single beats: the edges are where the
readings come from, and the sounds are where each of the three sounds
rises halfway up its own climb, used to recognise escapement faults, so
they sit near but not exactly on the edges. A sound line is missing when
that sound can't be told apart from its neighbour (a soft unlock merged
into the impulse, say), which is a finding in itself. The Tick tock
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
different amplitudes; the unlock figure is the one tg and commercial
timegraphers report. Above each plot are the beat count, the marks in ms
from that side's own drop, the unlock-to-drop time and the amplitude they
give. Linear (the
default) shows the envelope as the engine measures it; dB shows the level
below each side's loudest point, so the quiet unlock shows more clearly. Looking back, the pane shows the sound the session
kept nearest before that moment (one every 2 s); for a recording analysed
all at once it is worked out from the file when you stop on a moment.
When the tick's and tock's marks disagree, or a mark sits mid-ramp, the readings
built on them are suspect.

**Paper Strip.** One dot per beat, tick and tock in two colours. Across the
strip is how early or late each beat came against a perfect clock beating
exactly at the nominal beat rate (every 125 ms at 28,800 bph): a beat
before that clock's beat is early, after it late, the words Witschi uses.
Along it is time; both axes are labelled, in ms across and in minutes and
seconds along, and every grid line is drawn alike. **Rate Line** (on by
default) draws the Rate reading as a thin solid red line over the beats it was
fitted to, on top of them, as tg does: if the dots follow it, the reading
describes them well. **Parallel Guides** (on by default) adds faint red
lines at the same slope across the whole strip, one per grid step, since
the eye judges whether lines are parallel far better than it judges a
slope; dots that bend away from the guides show the rate changing. Under
**Draw over the strip**, amplitude (purple, on by default), rate (green)
and beat error (gold) readings can be drawn as lines against the same
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
unlock and from the drop) over time. **Time Span**, in the Charts and
Distributions cards, sets what they cover: **Session**, everything since the
session started, or **Strip**, the same stretch of time as the paper strip,
moving with it, so that with the charts under a strip lying across every
time axis lines up: the strip and the charts keep the same width of axis
on the left and of overlay scales on the right, so their times sit one
above the other. In Strip mode the strip sets the time, so only the
vertical scale can be dragged or zoomed. Wherever the pointer is on the
strip or a chart, a thin line marks that moment on all of them. With the pointer
anywhere over a chart, a vertical line marks that moment and a box lists
every line's value there. Their time axes move together:
the mouse wheel zooms time around the pointer, Ctrl and the wheel zooms
the vertical scale, dragging pans, and a double click fits the whole
session again and, while listening, follows new beats as they come in.
Once a chart has been panned or zoomed a **Follow Live** button
(**Show Whole Session** for a recording) appears on it and does the same,
and the box at the pointer lists these controls. Clicking a moment shows it on the strip and in the
readings. The scale starts at the 2nd to 98th percentile of the values so
one glitch doesn't flatten the lines. Changing the averaging time or the
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

**Remembered settings.** The app keeps its views between runs: the strip's
direction, width and length and what is drawn over it, the profile's
layout, scale and Time From, the panes shown and their arrangement, the
distribution settings, the averaging time, folded cards, the sidebar, the
appearance, the last microphone (used again when it is plugged in) and the
window's size and place. Nothing about the watch is kept: the lift angle,
beat rate and position start fresh, since the next watch on the pickup may
differ. **Reset Panes** puts the panes back as they started.

**Appearance.** Under Window: Auto follows the desktop's light or dark
setting; Light and Dark override it.

**No input touched at launch.** Listing the inputs asks each one what it
supports, which briefly opens it and can disturb another program recording
from the same microphone (a long take running beside the app, say). So the
app lists them only when the microphone menu opens, **Rescan** is pressed
or **Start** is clicked; until then the menu names the microphone kept from
last time, or System Default. Replaying or analysing a recording never
touches an input.

**Level meter.** In the toolbar, while listening to a microphone: a bar to
the loudest sample of the last half second, green when it is right, amber
when it is very quiet or too hot, red when the input clips, with marks at
the −10 dBFS target and the −6 dBFS limit, and the peak in dBFS beside it.

**Status bar.** The input; the signal, how far the typical beat's peak
stands above the typical level between beats, with a coloured dot and
what it means: 10× and above good; 5 to 10× fair, the rate reliable; 3 to
5× poor, only the rate to be trusted; below 3× no watch heard (noise alone
reads about 2×); its hint says what to do about a low figure. It measures
loudness, not the shape of the tick, so hum, knocks or a holder that
smears the sound aren't caught by it. Then how much sound the session
holds and whether it is saved, and on the right the beat rate and the
position.

## Settings

- **Input.** In the toolbar: a sound input device, or a WAV or FLAC file chosen with
  **Open…** or typed in. On Linux the menu offers each microphone once:
  **System default (via PipeWire)**, which is whatever input the
  desktop's sound settings choose and works alongside other programs, and
  each card's direct device, marked "(direct)", which goes straight to
  the card but can't open while the desktop sound server holds it. With a
  USB timegrapher microphone set as the desktop's input, choose the system
  default. **Show every input** lists everything the system offers, with
  ids, as `timegrapher devices` does (the switch is in the Microphone card). If the input can't be opened, stops,
  or sends no sound for 3 s, the app stops and says why above the
  readings, rather than showing an empty strip. The status bar shows the
  sample rate and bit depth the input opened at. Where the desktop passes
  dropped files on (X11, Windows, macOS; not yet Wayland), dropping a file
  on the window replays it. The app asks the device for 48 kHz in as few
  channels as it offers, mixed to mono, and takes 16-bit samples when the
  device has them.
- **Input level.** In the Microphone card: the microphone's gain and a peak meter
  with marks at the −10 dBFS target and the −6 dBFS limit. On Linux, for
  the system default the level is the sound server's input volume
  (`wpctl`, or `pactl` on PulseAudio), which the server writes back onto
  the microphone each time it opens it, so setting the card's own control
  there would not last; for a direct device it is the card's capture
  control (`amixer`). The change is made when you let go of the slider.
  On other systems, set the level in the system's sound settings. The
  same code (`timegrapher_core::mixer`) gives `timegrapher doctor` its
  proposals. A microphone's automatic gain chases every tick, so it
  clips and changes the shape of the sound; if it is on, the card shows
  an amber warning with **Turn it off**, and otherwise says nothing about
  it.
- **Beat rate.** Auto (guessed from the first seconds) or any standard rate
  from 12,000 to 72,000 bph. Changing it starts the readings again.
- **Lift angle.** 52° by default. Type the calibre's angle and press Enter,
  or pick a common one. Amplitude is worked out again at once, history
  included.
- **Position.** DU, DD, CU, CD, CL, CR. Changing it starts the readings
  again, since a new position is a new measurement, and is noted in the
  saved recording. Guided runs through the positions (as `timegrapher
  session` reports them) will build on it.

## Sessions

**Start** begins a session from the microphone. **Pause** stops listening
and keeps everything: the readings, strip, charts and the sound so far.
**Resume** carries on in the same session: the strip and charts leave
the paused time out, and the readings start from the beats after the
pause. **New session**, shown while paused, clears everything for the
next watch or position. Everything heard is kept as it comes in, so
**Save…** can be pressed at any time, running or paused: it asks for a
folder and copies the session there, and **Save again…** later brings
that copy up to date. Sound that hasn't been saved is never thrown away
without asking: New session and closing the window ask whether to save it
first.

Each saved recording is a folder named after the start time, the watch
and the position, so it can be analysed again later with a newer engine,
holding:

| File | Contents |
| --- | --- |
| `audio-001.wav`, ... | The audio as captured, mono, 16-bit for a 16-bit microphone, in files of at most an hour |
| `clock.csv` | System time against frames captured, every 10 s |
| `session.json` | Watch, position, lift angle, beat rate, device, start and end, position changes, and the readings at the end |

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
offer them too.

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
