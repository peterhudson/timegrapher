# Setting up: software, microphone and the test

A timegrapher is only a stopwatch with very good ears. If the ears are
deaf, deafened or listening to the fridge, every number after that is
rubbish. Get this right first and you'll save yourself a lot of puzzling.

**Ask before changing any setting on the person's computer.** Say what
you want to change, why, and how to put it back. Wait for a yes.

## 1. Getting the program

The program is a command-line tool, `timegrapher`, built from the Rust
source.

```sh
timegrapher --version          # already installed?
```

If not:

1. Install Rust if needed (https://rustup.rs; ask first, it installs into
   the user's home directory).
2. In the repository: `cargo build --release`.
3. The binary is `target/release/timegrapher`. Either use that path or copy
   it somewhere on the PATH (ask first).

Live sound input (`devices`, and `doctor` listening to a microphone) is
the default `live` feature. On Linux it needs the ALSA development package
and `pkg-config` (`libasound2-dev` on Debian/Ubuntu, `alsa-lib-devel` on
Fedora; ask before installing). If that can't be installed,
`cargo build --release --no-default-features` builds without live input:
`analyze`, `shape`, `long` and `doctor --file` still work on recordings
made with other tools. On Linux, `doctor` also uses `amixer` (package
`alsa-utils`) to read and set the mixer.

## 2. The microphone

### What works

- **A timegrapher microphone with a clamp** (the stand that comes with a
  Weishi, or one of the cheap USB stands). The watch is held in the clamp
  and the sound travels through the clamp into a contact pick-up. This is
  by far the best option.
- A contact (piezo) pick-up with a preamp, the case back pressed against it.
- A phone or laptop microphone will sometimes pick up a loud watch in a
  silent room, but rate is all you'll get with any confidence; the
  amplitude needs the faint unlock sound, which gets lost.

Sample rate: 44.1 or 48 kHz is fine. The tick's useful sound is in roughly
the 2–20 kHz band (check), so anything lower than 44.1 kHz throws some of it
away.

### Worked example: the cheap USB C-Media stand

A very common cheap USB timegrapher microphone is a C-Media USB audio
device. It only does **48 kHz, 16-bit, mono**, and it ships with
**automatic gain control on and the mic level at maximum**. Automatic gain
pumps the level up between ticks and squashes the ticks themselves; full
level clips every tick flat. You can still get a rate out of that, but the
amplitude and the tick shape are garbage.

The fix that worked on this project's own stand:

```sh
amixer -c <card> sset 'Auto Gain Control' off
amixer -c <card> sset Mic 10        # about 10 of 16 steps
```

`timegrapher doctor` on such a stand reports `agc_suspected` and
`clipping` and proposes exactly these commands.

### Finding the device

```sh
timegrapher devices          # every input, its sample rates, formats, channels, and the default
timegrapher devices --json
```

Pick the timegrapher stand, not the laptop's built-in microphone. A
USB stand usually shows up under the chip maker's name ("USB PnP Sound
Device", "C-Media", "USB Audio") rather than "timegrapher".

### Checking the signal with `doctor`

With the watch clamped, wound and running:

```sh
timegrapher doctor --device "USB PnP Sound Device" --seconds 5
timegrapher doctor --card 2 --json           # Linux: by ALSA card number
timegrapher doctor --file take.wav           # check an existing recording
timegrapher doctor --device NAME --save check.wav   # keep the few seconds it heard
```

It reports peak and RMS level (dBFS), clipped samples, the background
level between ticks, the tick-to-background margin, whether the
background rises between ticks (the sign of automatic gain), the beat rate
it detected and beats found against expected, a quick rate and beat error
(a few seconds only; don't read the watch from it), and a list of issues.
Each issue is a **fault** (spoils measurements) or a **warning** (worth
fixing; readings still usable). On Linux it also reads the card's mixer
(`amixer`) and shows the level and auto-gain controls.

Exit code **0** means nothing spoils a measurement (there may still be
warnings), **3** means at least one fault, **1** an error. The JSON
`verdict` is `ok` or `needs_attention`.

It changes nothing unless you give `--apply`, and **you only give
`--apply` after the person has agreed** to the specific fix it proposed.
Every proposed fix carries `requires_consent: true` for that reason.
`--apply` only runs commands (on Linux, `amixer`); on macOS and Windows
the fixes are instructions for the person to follow. After applying, a
live `doctor` checks again and reports the result under `after`.

| Issue | What it means | Usual fix |
| --- | --- | --- |
| `silent` (fault) | Nothing, or digital silence. | Wrong device; device muted; mic not plugged in; on macOS the terminal hasn't been given microphone permission. |
| `too_quiet` (warning) | Peaks below about −30 dBFS. | Raise the mic level; clamp the watch firmly; make sure the case touches the pick-up. |
| `clipping` (fault) | Samples at full scale, or flat-topped peaks below it (clipping in the analogue stage). Amplitude and shape will be wrong. | Lower the mic level (it suggests about −6 dB and a re-check); turn off any "boost". |
| `hot` (warning) | Peaks within 1 dB of full scale, not yet clipping. | Lower the level a step or two; a fully wound watch at high amplitude is louder. |
| `agc_suspected` (fault) | The background rises by more than 3 dB through the gap between ticks: automatic gain turning down on each tick and back up in the gaps. | Turn off auto gain / AGC / "audio enhancements". |
| `no_ticks` (fault) | Fewer than half the expected beats, or ticks less than 6 dB above the background. | Watch stopped? Not touching the mic? Very noisy room? Wrong device? Wrong beat rate (give `--bph`)? |
| `noisy` (warning) | Ticks found but less than 20 dB above the background: hum, fans, rubbing, handling. | Unplug mains chargers near the mic; move away from fans and the computer; don't touch the stand; try `--notch` for steady tones. |

Good signal: tick peaks around −10 dBFS (the level `doctor` aims for, with
room for a louder watch), no clipped samples, the background flat between
ticks, ticks 20 dB or more above it, and all the expected beats found.
`suggested_gain_change_db` in the JSON says how far to move the level.

### Per-OS mixer advice

Always ask before changing any of these, and tell the person how to undo it.

**Linux (ALSA, PipeWire, PulseAudio)**

```sh
arecord -l                         # list capture cards; note the card number
amixer -c 2 scontrols              # the controls this card has
amixer -c 2 sget Mic               # current level
amixer -c 2 sset 'Auto Gain Control' off
amixer -c 2 sset Mic 10            # set a level (steps or a percentage, e.g. 60%)
alsamixer -c 2                     # interactive: F4 for capture controls, M to mute/unmute
```

PipeWire and PulseAudio sit on top of ALSA and keep their own software
level per source. If the ALSA level is right but the recording is still
quiet or loud, check `wpctl status` / `wpctl get-volume <id>` (PipeWire) or
`pactl list sources short` / `pactl set-source-volume <name> 100%`
(PulseAudio). Set the software level to 100% (0 dB) and do the adjusting
in the ALSA hardware control, which is where the clipping happens.
`alsactl store` makes ALSA settings survive a reboot (ask first; it may
need root).

**macOS**

- *Audio MIDI Setup* (Applications › Utilities): select the USB microphone
  in the left list, choose the Input tab, set the format to 48,000 Hz and
  bring the input volume slider down until `doctor` stops reporting
  clipping. Many cheap USB mics have no hardware AGC switch on macOS; if
  `agc_suspected` persists, there's not much to do but reduce the level.
- *System Settings › Sound › Input* has a second input level slider.
- *System Settings › Privacy & Security › Microphone*: the Terminal (or
  whichever app runs `timegrapher`) must be allowed, or the input is
  silent.
- Turn off *Voice Isolation* / *Mic Mode* effects if offered; they're
  processing, and processing destroys tick shape (check: these modes apply
  to apps using the system voice-processing path; a plain recording may not
  be affected).

**Windows**

- *Settings › System › Sound › Input*: choose the device, then its
  *Properties*. Set the input volume, and turn **off "Audio enhancements"**
  (on Windows 11 this is a drop-down; on older versions an "Enhancements"
  tab with "Disable all enhancements").
- In the older *Sound control panel* (`mmsys.cpl`), Recording tab ›
  device › Properties: the *Levels* tab (level and any "Microphone Boost"),
  *Enhancements* (disable all), *Advanced* (set 1 channel, 16 bit, 48000 Hz
  if offered, and untick "Allow applications to take exclusive control" only
  if another app is grabbing the device).
- *Settings › Privacy & security › Microphone*: let desktop apps use the
  microphone.
- Some USB mics also expose "AGC" in the *Levels* or *Custom* tab.

## 3. Physical set-up

- **Clamp the watch firmly** in the stand's jaws, case against the
  microphone. A watch that can rattle in the clamp gives a scattered trace
  and odd tick shapes. Don't over-tighten on a thin case or crystal.
- Strap or bracelet out of the way so it doesn't rest on the desk or touch
  the stand.
- **Quiet room.** No fans, no fridge compressor, no washing machine. Put
  the stand on a soft mat, not directly on a desk that hums.
- **Mains-powered gear causes hum.** Laptop chargers, monitors, LED lamp
  drivers near the stand or on the same USB hub. Try the laptop on battery.
  `--notch 50,100,150` (or 60, 120, 180 in North America) for steady hum;
  steady high whines can be notched too.
- **Don't touch anything** while it records. Every knock is a false tick.
- Quartz watches won't work; there's no escapement to hear.

## The test

### Wind and settle

- **Fully wind.** A manual-wind watch: wind until you feel it stop, gently,
  never force. An automatic: 30–40 turns of the crown is roughly full for
  most (check per calibre; some need more), or a session on a winder. A
  fully wound watch gives the reference figures; a run-down one reads low
  and is a different test.
- **Let it settle.** After winding or handling, give it a few minutes
  running before the first reading. Witschi's procedure runs the watch for
  about 20 minutes before measuring (check this is worth asking of a
  layperson). After each position change, wait 30 s to a minute; amplitude
  takes a while to settle, especially going from horizontal to vertical.
- Note the time of winding. The 24-hour test is the same positions a day
  later without winding again.

### Positions

| Name | Witschi | Orientation |
| --- | --- | --- |
| Dial up | CH | Lying flat, face up |
| Dial down | CB | Lying flat, face down |
| Crown down | 9H | Standing on edge, crown at the bottom (9 o'clock up) |
| Crown left | 6H | 6 o'clock up |
| Crown up | 3H | 3 o'clock up |
| Crown right | 12H | 12 o'clock up |

(For a watch with the crown at 3 o'clock; for a left-hand-crown or 4 o'clock
crown, name the position by the dial number that's up.) COSC tests five
positions (check which five); Witschi's procedure uses all six and starts
dial down.

### How long to record

| Purpose | Length |
| --- | --- |
| Quick health check | 60 s dial up after settling |
| Positions | 60–120 s per position, after 30–60 s settling |
| Fourth-wheel (once a minute) faults | 30 min or more; an hour or two is better |
| Centre wheel (once an hour) | 6 hours or more |
| Barrel, isochronism | 24 hours or the whole power reserve |

### Recording

Until a `record` command exists, record with the system's tools and analyse
the file. 48 kHz, 16-bit, mono, WAV or FLAC.

```sh
# Linux (ALSA): card 2, 2 minutes
arecord -D plughw:2,0 -f S16_LE -r 48000 -c 1 -d 120 dialup.wav

# macOS or Linux with SoX installed: default input, 2 minutes
sox -d -r 48000 -c 1 -b 16 dialup.wav trim 0 120

# Windows or macOS with ffmpeg (list devices first)
ffmpeg -list_devices true -f dshow -i dummy                         # Windows
ffmpeg -f dshow -i audio="Microphone (USB PnP Sound Device)" -ac 1 -ar 48000 -t 120 dialup.wav
ffmpeg -f avfoundation -list_devices true -i ""                     # macOS
ffmpeg -f avfoundation -i ":1" -ac 1 -ar 48000 -t 120 dialup.wav
```

Audacity also works: set the project rate to 48000, mono, record, export
as WAV (16-bit PCM).

`timegrapher doctor --save` keeps the few seconds it checked; that's for
checking the signal, not for measuring.

**For a long run, keep a clock log** so the sound card's own clock error
can be removed (see [long-runs.md](long-runs.md)). On Linux:

```sh
arecord -D plughw:2,0 -f S16_LE -r 48000 -c 1 run.wav &
while sleep 60; do echo "$(date +%s%N),$(stat -c %s run.wav)"; done > run-clock.csv
```

(One line a minute of nanosecond time against bytes written; only the
slope matters, so the WAV header doesn't hurt. The system clock must be
NTP-synchronised. A WAV file is limited to 4 GB, about 12 hours at
48 kHz 16-bit mono; for longer runs record in segments or to
FLAC.)

