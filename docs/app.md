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

**Readings.** Rate (s/d), amplitude (degrees) and beat error (ms), each a
fit over the last few seconds of beats (2 to 60 s, 10 s by default, the
same choices as a Witschi). Under the amplitude are the two sides
separately, A (even beats) and B (odd beats); which side is the entry
pallet can't be told from the sound, so the sign of the beat error follows
which beat came first, as on tg. Below the readings: the beat rate, the
jitter (how much single beats scatter about the fit) and how many beats the
readings used.

**Paper strip.** One dot per beat, tick and toc in two colours. Across the
strip is how early or late each beat came against a clock running at the
nominal beat rate; down the strip is time, newest at the top. A watch on
rate draws a vertical line; a gaining watch leans right as it rises (/),
a losing one leans left (\\). The gap between the two lines is the beat
error. A line that runs off one edge comes back on the other, so the
strip's width (±1 ms to ±62.5 ms) is its zoom, and its length (10 s to 2 h)
sets how much history it shows. **Centre** puts the latest beats back on
the centre line; **Clear** starts the readings and the strip again.

**Rate and amplitude over time.** Beside the strip, and full size on the
second tab: the readings over the whole session, so a slow drift or a
change after turning the watch shows at a glance.

**Status bar.** The input, its peak level in dBFS over the last half second
(red when the input clips, amber when it is very quiet; aim for ticks
peaking around −12 dBFS), how far the beats stand above the noise ("no
watch heard" when they don't), and the recording in progress.

## Settings

- **Input.** A sound input device (the default first; Rescan looks again), or a
  WAV or FLAC file. Dropping a file on the window replays it. The app asks
  the device for 48 kHz in as few channels as it offers, mixed to mono,
  and takes 16-bit samples when the device has them.
- **Beat rate.** Auto (guessed from the first seconds) or any standard rate
  from 12,000 to 72,000 bph. Changing it starts the readings again.
- **Lift angle.** 52° by default. Changing it rescales the amplitude at
  once, including the history, since amplitude is proportional to it.
- **Position.** DU, DD, CU, CD, CL, CR. Changing it starts the readings
  again, since a new position is a new measurement, and is noted in the
  saved recording.

## Saving recordings

With **Save the recording** ticked, every session is kept so it can be
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
