# 25. The Command Line

**What it is:** `timegrapher`, a program run from a terminal, which
measures with the same engine as the app. It reads a recording, or
records one, and prints what it measured as text or JSON (JavaScript Object Notation, a
text format programs read).

**Why you'd use it:** to read many takes at once, to check a take
outside the app, to put figures in a script or a spreadsheet, or to let
an AI agent drive the timegrapher. Every figure the app shows comes from
the same code, so the two agree on the same sound and settings.

## Where It Is

Every download holds it, in a folder named **Command Line**, beside
`timegrapher-mcp` (chapter 26).

| System | How to run it |
| --- | --- |
| Mac | `/Applications/Timegrapher.app/Contents/MacOS/timegrapher`. [Installing](../reference/install.md#macos) shows how to make `timegrapher` run by name. |
| Windows | Open a terminal in the **Command Line** folder and run `.\timegrapher.exe --help`. |
| Linux | `"Command Line/timegrapher"` in the unpacked folder, or `./Timegrapher.AppImage cli` followed by the command. |

`timegrapher --help` lists the commands, and `timegrapher analyze --help`
gives one command's options. `timegrapher --version` gives the version.

## The Commands

**Reading a recording**

| Command | What it does | When you'd use it |
| --- | --- | --- |
| `analyze FILE` | Reads one WAV or FLAC whole: rate, amplitude, beat error, jitter, clipping, the three sounds, and periodic components. `--beats`, `--windows` and `--trend` write CSV files of each beat, each window and the readings over time. | The first look at any recording. |
| `session PATHS` | Reads a watch measured in several positions, from a take's `session.toml` or files with the position in their names, into one report per position, with the Positions Test figures (M8). `--init` writes a `session.toml` to edit. | A positions test made with `record`, or any take, read as the app's table reads it. |
| `long FILES` | Reads hours to days: rate and amplitude over time and the periodic changes that point at a wheel of the train, with a report folder. Several files, or a folder, are read as one recording. | A take of many hours, such as a power reserve run. |
| `series FILES` | The structure of each series of readings: autocorrelation, Ljung–Box, Allan deviation, CUSUM, change points, a trend, a period search and two levels, each with its statistics (M9, M10). | What Steadiness shows, as figures, for a script or a second look. |
| `shape FILE` | Times the three sounds of each side's beat. | When the amplitude or beat error looks wrong. |
| `profile FILE` | The Tick and the Tock over a stretch (`--at`, `--span`), with the unlock, drop and sound marks; `--svg` draws them. | The Tick Tock Profile pane (chapter 7), for one stretch. |

**Recording and the microphone**

| Command | What it does | When you'd use it |
| --- | --- | --- |
| `devices` | Lists the sound inputs and the rates and formats each takes. `--agc` reads a USB microphone's automatic gain; `--agc-off` turns it off (Mac and Windows). | To find a microphone's name for `--device`. |
| `doctor` | Records a few seconds and reports the level, clipping, the background between ticks, the signal above noise and whether a regular beat is found, with the mixer changes it would make. Nothing changes without `--apply` (Linux). | When the app hears nothing, or clips (chapter 3). |
| `clock measure` | Times a sound input against the computer's clock, kept by NTP. Twenty minutes gives its error to a fraction of a ppm. `clock list`, `set` and `forget` manage what is stored. | Once per sound input, so every rate is corrected for the card's clock (M3). |
| `record FOLDER --position DU` | Records one position into a session folder, as the app saves a take, checks it and adds it to `session.toml`. `--power-reserve` records until the watch stops. `--stop-file PATH` ends it early once that file appears, as Stop does in the app. | A positions test from a script, or with no screen. |

**Watches, calibres and the rest**

| Command | What it does | When you'd use it |
| --- | --- | --- |
| `watches list`, `watches show NAME` | The watches in the app's Watches folder, and each one's takes with the folder to pass to `session`, `long` or `series`. It only reads. | To find a take's folder. |
| `watches new`, `edit`, `add-take`, `move-take`, `keep-analysis-only`, `delete-take`, `add-photos`, `delete-photo` | What the Watches view does: add or edit a watch, add a take folder to a watch, move one, keep its analysis only, delete one, add or delete photos. Close the app first; adding a take works while it is open. | To file takes from another program, such as a shop's records, or from a script. |
| `calibres` | The calibre table, with what the app added (chapter 24). `--search "dd 3235"` finds one; a name prints its entry. | To check a calibre's beat rate, lift angle and wheels. |
| `report` | Makes a zip to attach to an issue (chapter 27). | When something looks wrong. |
| `excerpt RECORDING --out FILE.flac` | Writes a stretch of a recording or take (`--from`, `--to`, in seconds) as one FLAC file, its samples untouched, and never over a file already there. `--json` gives its size and SHA-256. | To keep the sound of a test beside its readings, in another program. |
| `update` | Asks GitHub whether a newer release is out. Nothing is downloaded. | To check by hand. |
| `regress ROOT` | Measures every reading of every take under a folder again (holdout takes are left out; `--holdout` reads only them, against tg, as counts) and compares it with the figures tg, another open-source timegrapher, gave for the same file, and with a stored baseline; exits 3 when one moved beyond its margin. `--fetch` downloads the public test recordings' sound first, each checked by its SHA-256 (Secure Hash Algorithm) fingerprint, and refuses the whole list if any name in it is a path. | To check that a new build still reads the test recordings as before ([Regression check](../developer/regress.md)). |
| `synth` | Writes a synthetic watch recording. | For the project's own testing. |

## Options Most Commands Share

| Option | What it does |
| --- | --- |
| `--calibre "Dandong 3235"` | Takes the beat rate, lift angle and wheels from the calibre table. An unknown name lists the known ones. |
| `--bph 28800` | The beat rate. Left out, it is found from the beats. |
| `--lift 52` | The lift angle in degrees: the calibre's, else 52. The amplitude depends on it (M4). |
| `--device NAME` | Corrects the rate with the clock error stored for that input. |
| `--notch 5000,6000` | Takes out steady tones, such as a hum or whine near the stand. |
| `--highpass 1500` | The high-pass corner in Hz. |
| `--json` | Prints one JSON document instead of text. |

## An Example

```text
$ timegrapher analyze take_part01-001.flac --calibre "Dandong 3235"
Calibre      Dandong 3235: 28800 bph, lift 59°, as set unless a flag said otherwise
Take         …/session.toml lists this file; `timegrapher session` reads it:
             Dial Up (DU) from 20 to 600 s: rate -0.7 ± 0.4 s/d, amplitude 282 ± 1°, beat error 0.24 ± 0.04 ms
Whole file   read alone, with no settling and none of the take's settings:
Duration     600.0 s at 48000 Hz
Beat rate    28800 bph, 4711 beats found (98.1% of those expected)
Rate         -0.7 ± 0.3 s/d, corrected for the sound card's clock (18.9 ppm slow, …); +0.9 s/d on the card's clock
Beat error   0.24 ± 0.04 ms (from the unlock; 0.49 ms from the drop)
Jitter       258 us per beat (within 10 s windows)
Rate spread  -3.0 to 4.3 s/d (5th-95th percentile of 10 s windows)
Amplitude    303 ± 1 deg (Tick 292, Tock 309; lift angle 59 deg)
Clipping     on 0.0% of beats (0 of 4711, 210 samples), peak 0.0 dBFS
```

This is ten minutes of a Dandong 3235, dial up. The file is part of a
take, so the first lines give the take's own reading, as the app and
`session` read it, after its 20 s settle. The rest reads the file alone,
from its first beat and without the take's settings, so its figures can
differ from the take's; the take's are the ones the app shows. The rate
is corrected
for the sound card's clock from the USB lines in the recording (M3), and
gives the uncorrected figure too.

## For Scripts and Agents

- `--json` prints exactly one JSON document, starting with `schema`
  (such as `timegrapher.analyze/2`), the software's version and the
  input it read. Within a schema version, fields are only added.
- **Exit codes:** 0 when it worked, 1 for an error (the message goes to
  the error stream), 3 from `regress` when a take moved beyond its
  margins. `record` and `doctor` exit 0 whatever they hear: no beat or
  clipping is reported, not refused.
- `doctor` and `record` open the microphone only while they run.
  `doctor` changes the computer's settings only with `--apply`, and
  `devices` only with `--agc-off` or `--agc-on`; nothing else changes
  them.
- No command grades a watch or gives advice; each reports what it
  measured.

[The agent interface](../reference/agent-interface.md) gives every JSON field.

## What It Can't Tell You

- **More than the sound holds.** It reads the same sound the app reads,
  with the same limits.
- **A setting you didn't give.** A recording with no `session.toml` is
  read at 52° and the beat rate found unless you pass `--calibre`,
  `--lift` or `--bph`.

---

Previous: [Settings and Calibres](24-settings-and-calibres.md) ·
Next: [Claude Desktop](26-claude-desktop.md)
