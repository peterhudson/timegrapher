# 3. Setting Up the Microphone

Every figure Timegrapher reports comes from the sound. A microphone that
is too loud, too quiet, or turning its own level up and down between
beats gives figures that describe the microphone as much as the watch.
This chapter is about getting a clean sound, and about the checks the
software makes so you can tell when you haven't.

## The Microphone Section of the Watch Pane

The top of the **Watch** pane is the **Microphone** section. Choose
**Microphone** (rather than **Recording**) to listen live.

![The Microphone section while listening: the microphone menu, Stop and Save… buttons, Input Level and the Peak meter reading −9 dBFS](images/microphone-listening.png)

- **The microphone menu** lists the computer's sound inputs. The app looks
  for them only when you open the menu, press **Rescan** or press
  **Start**, never when it starts: asking an input what it can do briefly
  opens it, which could disturb another program recording from it. Until
  then the menu shows the microphone you used last, or **System Default**.
- **Rescan** looks for inputs again, after you plug one in.
- **Start** begins listening. While listening it becomes **Stop**, and
  after stopping, **Resume**.
- **Input Level** is the microphone's gain (see below).
- **Peak** is a meter showing the loudest sample of the last half second,
  with its level written under it in dBFS: decibels below the loudest
  sound the input can carry. It has marks at −10 and −6 dBFS. It turns red
  the moment any sample clips.
- **Show Every Input** lists everything the system offers, with the
  system's own names for them.

The app asks the input for 48 kHz and, where the input offers it, 16-bit
samples, mixed to one channel. The status line at the bottom of the
window says what it opened at.

### Which Input to Choose on Linux

On Linux the menu offers each microphone in two ways:

- **System default (via PipeWire)** (or "via PulseAudio") is whatever
  input your desktop's sound settings choose, and works alongside other
  programs. If your timegrapher microphone is set as the desktop's input,
  choose this.
- Each sound card's own device, marked **(direct)**, goes straight to the
  card. It cannot open while the desktop's sound server is using the card.

On macOS and Windows, choose the microphone by name.

## Setting the Level

The level is right when the beats are well above the background noise
and the loudest part of each beat stays below the top of the input's
range.

- **Too loud**: the loudest part of each beat is cut flat (**clipping**).
  The peak meter turns red and an amber banner above the readings says
  **Clipping on 3% of beats** (or whatever share it is). Clipping
  flattens the drop, which moves where the drop seems to be, so the
  **amplitude and beat error read wrong**. The rate comes from when the
  beats arrive, which clipping doesn't change. The banner follows the
  readings: it shows while the beats being averaged clip and goes when
  they no longer do.
- **Too quiet**: the beats sink towards the background. The **signal**
  figure in the status line (see below) falls, the jitter grows, and in
  the end no beats are found.

`timegrapher doctor` (below) suggests a level that puts the loudest
sample at about −10 dBFS, the meter's first mark, leaving room for a
louder beat.

**How to change the level** depends on the computer:

- **Linux**: drag **Input Level** in the Watch pane. The change is made
  when you let go of the slider. For the system default it sets the sound
  server's input volume; for a direct device, the card's own capture
  control. When neither can be set from the app, it says **not adjustable
  here**, and you set the level in your desktop's sound settings instead.
- **macOS**: System Settings → Sound → Input, then the input volume.
- **Windows**: Settings → System → Sound → your microphone's properties,
  then the input volume.

## Automatic Gain

Many inexpensive USB timegrapher microphones arrive with **automatic gain
control** turned on and the level at maximum. Automatic gain turns the
level up in the quiet between beats and down again on each beat, which
reshapes the very sound the amplitude is worked out from.

When the app can see the microphone's automatic gain switch and finds it
on, the Microphone section says **Automatic gain: on** with a **Turn It
Off** button. When it is off, the app says nothing about it. On systems
where the app can't see the switch, `timegrapher doctor` (below) looks
for the background rising between beats, the sign of automatic gain at
work, and reports it.

## The Signal Figure

The status line at the bottom of the window shows **signal**, such as
"signal 30×": how far the typical beat's peak stands above the typical
level between beats.

![The status line: the input and its format, signal 30×, the sound held so far, the beat rate and the position](images/status-line.png)

Noise alone reads about 2×. Below 4×, no beats are logged. The figure
measures loudness, not the shape of the beat, so hum, a knock or a holder
that smears the sound are not caught by it.

## When the Input Stops

If the input can't be opened, stops, or sends no sound for 3 seconds,
the app stops listening and says why above the readings, rather than
showing an empty strip.

## Checking the Microphone from the Command Line

`timegrapher doctor` listens for a few seconds, or reads a recording, and
states what it finds: the level, clipping, whether the background rises
between beats, the signal above the noise, and whether a regular beat was
found. On Linux it also reads the sound card's mixer controls and names
the exact setting to change. It changes nothing unless you add
`--apply`.

Here it is reading a recording that clipped on a few samples:

```text
$ timegrapher doctor --file test_watch_DU.flac
Source       test_watch_DU.flac
Level        peak 0.0 dBFS, RMS -34.0 dBFS, 66 clipped samples
Ticks        4775 of about 4800 beats at 28800 bph; tick -16.6 dBFS, background -45.1 dBFS
Signal       32× above the background
Gap rise     background rises -0.0 dB between ticks
Quick look   rate +48.8 s/d, beat error 0.1 ms (on the sound card's clock)
Detected     Clipping: 66 samples (66 samples clipped, peak 0.0 dBFS)
Changes --apply makes:
  - The peak is 6 dB above -10 dBFS; no capture control was found on the mixer to set it.
```

Run `timegrapher doctor` with no file to listen to the default input, or
`timegrapher devices` to list the inputs and `--device NAME` to choose
one. [The Command Line](12-command-line.md#doctor) has the details.

## Good Habits

These come from how the sound reaches the microphone, not from any rule
about the watch:

- Keep the watch in the same place in the clamp from one measurement to
  the next, so the sound reaches the pick-up the same way.
- Keep the table still. Typing, a mouse, a phone buzzing on the table or
  footsteps on a wooden floor all reach a contact microphone.
- Measure in the same position for a whole reading. Turning the watch is a
  new measurement; the app keeps positions apart (see
  [Positions](10-positions.md)).
- Close other programs that use the microphone before pressing **Start**,
  and stop listening before another program starts recording.

---

Previous: [Your First Measurement](02-first-measurement.md) ·
Next: [The Window](04-the-window.md)
