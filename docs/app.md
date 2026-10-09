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

Along the top, the toolbar: **Microphone** or **Recording**, the
microphone menu (or the file to open), the input's level meter and the
button that starts or stops (**Start**, **Replay**, **Stop**; **Analyse
all** for a recording). On the left, the settings in cards: **Watch**,
**Microphone** (only for the microphone), **Paper strip**, **View** and a
reminder of what the mouse does. In the rest, the three readings in cards
and below them the panes. Along the bottom, the status line. Every setting
explains itself when the pointer rests on its name. Before anything is
open, the main area says how to begin, with buttons to start listening or
open a recording; a problem shows in a red banner above the readings until
it is dismissed or the next start clears it. The app follows the
desktop's light or dark setting (Appearance, under View, overrides it),
and uses the Inter typeface with figures of even width, so readings don't
shift as they change.

**Readings.** Rate (seconds per day gained, +, or lost, −), amplitude (degrees) and beat error (ms), each a
fit over the last few seconds of beats (10 s by default; type any time
from 1 s to an hour, or pick one of Witschi's 2 to 60 s). The big amplitude is the average of the two
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
Under the rate: how many beats the readings used and
the jitter: how far single beats land from the steady line the rate and
beat error are fitted to (a robust standard deviation, in microseconds),
which is the spread of the dots across the strip. Lower is steadier; it
rises with noise or a muffled sound as well as with a watch that runs
unevenly, so compare it on the same stand and microphone. Hover over any
reading for a short explanation.

**Panes.** Below the readings are five panes: the paper strip, the tick tock
profile, and charts of rate, amplitude and beat error over the session. Drag a pane by its tab
to put it beside, above or below another, or onto another's tab to stack
them; drag the gaps between panes to resize them. The × on a tab hides
that pane, and the switches under **View** in the sidebar hide or show each pane
and the readings across the top; a hidden pane comes back where it was,
and changing the strip's direction keeps the choice. **Reset panes**
shows them all in their starting places. Periodicity views will join
them.

**Tick tock profile.** The typical sound of the ticks (blue) and of
the tocks (orange) over the averaging time (Average over, up to 60 s, so
it describes the same beats as the readings; a longer time gives a
steadier shape, and after lengthening it the beat count fills in as beats
arrive), from the drop's side of the beat back
through the unlock, with a shaded band where the middle 80% of beats fall
and the marks the engine read: unlock (green) and drop (red), from which
amplitude and the unlock-based beat error come, the drop's peak (purple),
the three sounds where the engine tells them apart (dashed: 1 unlock,
2 impulse, 3 drop) and the noise floor. Each line is named on the plot.
Above each plot are the beat count, the marks in ms from the
beat, the unlock-to-drop time and the amplitude they give. Linear (the
default) shows the envelope as the engine measures it; dB shows the level
below each side's loudest point, so the quiet unlock shows more clearly. Looking back, the pane shows the sound the session
kept nearest before that moment (one every 2 s); for a recording analysed
all at once it is worked out from the file when you stop on a moment.
When the tick's and tock's marks disagree, or a mark sits mid-ramp, the readings
built on them are suspect.

**Paper strip.** One dot per beat, tick and tock in two colours. Across the
strip is how early or late each beat came against a clock running at the
nominal beat rate; along it is time. Running **down** (the default, as on
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
click returns to the newest beats and centres them. **Auto-centre** keeps
the newest beats on the centre line all the time, so the older ones slide
away from it as the rate wanders; sliding the trace by hand turns it off.
**Centre** centres once; **Clear** starts the readings and the strip again.

**Charts.** Rate, amplitude (the average, from the ticks and from the tocks) and beat error (from the
unlock and from the drop) over the whole session. With the pointer
anywhere over a chart, a vertical line marks that moment and a box lists
every line's value there. Their time axes move together:
the mouse wheel zooms time around the pointer, Ctrl and the wheel zooms
the vertical scale, dragging pans, and a double click fits the whole
session again and, while listening, follows new beats as they come in.
Once a chart has been panned or zoomed a **Follow live** button
(**Show whole session** for a recording) appears on it and does the same,
and the box at the pointer lists these controls. Clicking a moment shows it on the strip and in the
readings. The scale starts at the 2nd to 98th percentile of the values so
one glitch doesn't flatten the lines. Changing the averaging time or the
lift angle works every point out again, back to the start of the session.

**Appearance.** Under View: Auto follows the desktop's light or dark
setting; Light and Dark override it.

**Level meter.** In the toolbar, while listening to a microphone: a bar to
the loudest sample of the last half second, green when it is right, amber
when it is very quiet or too hot, red when the input clips, with marks at
the −10 dBFS target and the −6 dBFS limit, and the peak in dBFS beside it.

**Status bar.** The input, how far the beats stand above the noise ("no
watch heard" when they don't), the recording in progress, and on the
right the beat rate and the position.

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
- **Input level.** In the Microphone card: the microphone's gain, a switch
  for its automatic gain when it has one (keep it off), and a peak meter
  with marks at the −10 dBFS target and the −6 dBFS limit. On Linux, for
  the system default the level is the sound server's input volume
  (`wpctl`, or `pactl` on PulseAudio), which the server writes back onto
  the microphone each time it opens it, so setting the card's own control
  there would not last; for a direct device it is the card's capture
  control (`amixer`). The change is made when you let go of the slider.
  On other systems, set the level in the system's sound settings. The
  same code (`timegrapher_core::mixer`) gives `timegrapher doctor` its
  proposals.
- **Beat rate.** Auto (guessed from the first seconds) or any standard rate
  from 12,000 to 72,000 bph. Changing it starts the readings again.
- **Lift angle.** 52° by default. Type the calibre's angle and press Enter,
  or pick a common one. Amplitude is worked out again at once, history
  included.
- **Position.** DU, DD, CU, CD, CL, CR. Changing it starts the readings
  again, since a new position is a new measurement, and is noted in the
  saved recording. Guided runs through the positions (as `timegrapher
  session` reports them) will build on it.

## Saving recordings

With **Save the recording** switched on (in the Microphone card, before pressing Start), every session is kept so it can be
analysed again later with a newer engine. Each recording is a folder
named after the start time, the watch and the position, holding:

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
