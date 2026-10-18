# 12. The Command Line

`timegrapher` is the command line tool. It measures with the same engine
as the app, on recordings of any length, from a few seconds to days, and
writes reports you can open in a browser or send by email. It is also how
AI assistants drive the software (see [Claude Desktop](13-claude-desktop.md)).

You don't need it to use the app. It is worth knowing for:

- checking a microphone in detail (`doctor`);
- measuring the sound card's clock (`clock`);
- reading a multi-position test into a report (`session`);
- analysing long runs of hours or days (`long`, `series`);
- scripts, spreadsheets and other programs (every command has `--json`).

## Finding and Running It

The program comes with the app. Your download's **Start Here** page says
where it is; in short:

- **macOS**: inside the app, at
  `/Applications/Timegrapher.app/Contents/MacOS/timegrapher`. To run it by
  name from Terminal, link it once:

  ```sh
  sudo ln -s /Applications/Timegrapher.app/Contents/MacOS/timegrapher /usr/local/bin/timegrapher
  ```

- **Windows**: `timegrapher.exe`, in the folder you extracted. Open a
  terminal in that folder and run `.\timegrapher.exe`.
- **Linux**: `timegrapher` in the tarball's folder, or inside the
  AppImage: `./timegrapher-*.AppImage cli`.

Then:

```sh
timegrapher --help            # the list of commands
timegrapher analyze --help    # the options of one command
timegrapher --version
```

The examples below were run on recordings from the public
[test corpus](https://github.com/peterhudson/timegrapher-corpus).
Their figures illustrate the output and say nothing about the watch.

## Common Options

Most commands that read a recording take:

| Option | Means |
| --- | --- |
| `--calibre NAME` | The movement, from the calibre table (see `calibres`): its beat rate, lift angle and wheels |
| `--bph N` | Beat rate in beats per hour (found from the beats if left out) |
| `--lift DEG` | Lift angle in degrees (the calibre's, or 52 if left out) |
| `--notch 5000,7000` | Remove steady interference tones at these frequencies, in Hz |
| `--json` | Print one JSON document instead of text |

`--bph` and `--lift` win over the calibre's figures.

## analyze

Reads one WAV or FLAC recording in one position and states the rate,
amplitude, beat error and more:

```text
$ timegrapher analyze test_watch_DU.flac --calibre "Rolex 3235"
Calibre      Rolex 3235: 28800 bph, lift 55°, as set unless a flag said otherwise
Duration     600.0 s at 48000 Hz
Beat rate    28800 bph, 4802 beats found (100.0% of those expected)
Rate         +47.1 ± 0.7 s/d, corrected for the sound card's clock (19.2 ppm slow, from 8 USB lines in the recording, 0.0 ppm apart); +48.8 s/d on the card's clock
Beat error   0.27 ± 0.00 ms (from the unlock; 0.11 ms from the drop)
Jitter       111 us per beat (within 10 s windows)
Rate spread  30.7 to 75.1 s/d (5th-95th percentile of 10 s windows)
...
```

Each line is a measurement with its ± where it has one. **Rate spread** is
the range of 10-second rates across the recording. Further lines give the
amplitude, clipping, the beat's shape and any periodic changes found.
`--beats beats.csv` writes one row per beat, and `--windows windows.csv`
the rate, beat error and amplitude over time, for a spreadsheet.

## doctor

Checks the microphone: records a few seconds (or reads `--file`) and
states the level, clipping, whether the background rises between beats
(automatic gain at work), the signal above the noise and whether a regular
beat is found. On Linux it reads the sound card's controls and lists the
exact changes it would make; `--apply` makes them. It changes nothing
without `--apply`. See
[Setting Up the Microphone](03-microphone.md#checking-the-microphone-from-the-command-line)
for an example.

```sh
timegrapher devices                  # the sound inputs
timegrapher doctor                   # listen to the default input for 5 seconds
timegrapher doctor --device "USB"    # a named input
```

## profile

Draws the Tick and Tock sounds over a stretch of a recording, with the
unlock, drop and sound marks the engine measured: the same picture as the
app's Tick Tock Profile.

```text
$ timegrapher profile test_watch_DU.flac --at 60 --lift 55 --svg profile.svg
Stretch      60.0–70.0 s at 28800 bph, lift angle 55 deg; times in ms from the beat
Tick         40 beats; unlock -9.18, drop -0.53, peak +0.12; sounds -9.17 / -6.77 / -0.51; amplitude 253 deg
Tock         40 beats; unlock -9.26, drop -0.53, peak -0.04; sounds -9.26 / -6.36 / -0.48; amplitude 251 deg
```

`--at` and `--span` choose the stretch; `--svg` saves the picture.

## shape

Measures the three sounds of each side's beat across the whole recording:
when each rises and peaks, the intervals between them, their levels, and
how often each was found, for the Tick and the Tock separately. The
figures describe the sound; `timegrapher shape --help` and `--json` name
each one.

## calibres

The calibre table: beat rate, lift angle, escape wheel and each wheel's
time for one turn, with sources and notes, plus anything you added in the
app.

```text
$ timegrapher calibres 3235
Rolex 3235 (Rolex)
Also matches  3235, 3230, 3255, 3285
Lift angle    55°
Escape wheel  20 teeth
...
```

`timegrapher calibres --search "dd 3235"` lists the calibres that match,
best first. As in the app, read the notes and sources: see
[Calibres and Lift Angles](09-calibres.md#reading-a-calibres-entry).

## record

Records one position of a test into a test folder, as the app saves a take,
checks it at once, and adds it to the folder's `session.toml`:

```sh
timegrapher record mywatch/ --position DU --seconds 120 --calibre "ETA 2824-2" --wind-h 0
timegrapher record mywatch/ --position DD --seconds 120
...
timegrapher session mywatch/
```

It listens only while it runs. When no regular beat is found, or the beats
clip, it says so, and keeps the take.

## session

Reads one watch measured in several positions (and, optionally, at full
wind and 24 hours later) into a multi-position report in the style of
Witschi's SEQ mode. Give it a folder of recordings, or a folder made by
`record` or the app.

```sh
timegrapher session --init six-positions/    # writes session.toml to check and edit
timegrapher session six-positions/
```

`--init` guesses each recording's position from its file name
(`watch_DU.flac`, `watch_CL.flac` and so on); check them, and add the
watch, calibre, lift angle and `wind_h` (the hours since full wind). Then:

```text
Watch        Test watch, calibre Rolex 3235; 28800 bph, lift angle 55 deg

Position          Wind                  Rate   Amp    BE  drop  Jitter      10 s rates
                                         s/d   deg    ms    ms      us             s/d
Dial Up (CH)      not given            +48.6   252  0.27  0.11     110        +31..+75
Dial Down (CB)    not given            +43.6   241  0.05  0.19     155        +21..+57
Crown Down (9H)   not given            +25.2   233  0.15  0.20     161        +10..+34
Crown Left (6H)   not given            +66.9   236  0.21  0.31     157        +39..+84
Crown Up (3H)     not given            -15.8   224  0.30  0.27     111         -24..-7
Crown Right (12H) not given            -19.2   231  0.25  0.03     114         -24..-9
...
At full wind          X +24.9 s/d (H +46.1, V +14.3); D 86.1 s/d, 27 deg; DVH -31.8 s/d, -15 deg; Di +18.3 s/d
```

(Some columns are left out here.) The summary goes on to the characteristic
values (X the mean rate, D the largest difference, DVH vertical less
horizontal, and with a 24-hour half, Im and Ie), any two-state behaviour
in a position, and any periodic change found. It writes `report.html`,
one self-contained page to open in any browser or attach to an email,
with `summary.json` and `readings.csv` beside it, in `session_report/`.

[Test sessions](../sessions.md) explains the session file and each value
in full.

## long

Analyses a long recording, hours to days, in bounded memory: rate and
amplitude over time, the sound card calibrated against internet time, and
periodic changes named after the wheel whose turn they match. It writes an
HTML report.

```text
$ timegrapher long test-watch-2h --clock test-watch-2h/clock.csv --calibre "Rolex 3235" --lift 55
Recording    2:00:00 at 48000 Hz, 28800 bph, 57585 beats (100.0% clean)
Clock        sound card 19.67 ppm slower than NTP time, so uncorrected rates read 1.70 s/d fast; corrected from 119 entries, 36.0 ms rms
Rate         +16.54 s/d over the run; 18 s slices from 1.3 to 51.8 s/d (5th-95th pct)
Beat error   0.31 ms (from the unlock; 0.17 ms from the drop)
Amplitude    249 deg median; slices from 241 to 255 deg (lift angle 55 deg)
Periodic changes in amplitude:
      60.01 s ±0.50   swing 6.6 deg p-p      explains  24%  false alarm 7e-52    matches the fourth wheel
...
Report       test-watch-2h/take_part01-001_long/report.html
```

(Some lines are shortened here.) `--clock` reads the `clock.csv` a saved
take carries, to correct the sound card's clock. For `long`, give the lift
angle with `--lift`: the calibre names the wheels. [Long runs](../long-runs.md)
explains the report and the search.

## series

The same tests as the app's Steadiness views, on a recording or folder:
correlation between readings, level changes, a trend, a cycle and two
levels, each stated with its statistics.

```text
$ timegrapher series test-watch-2h --calibre "Rolex 3235" --lift 55
Rate repeats every 60 s (the fourth wheel), 11.7 s/d peak to peak. Its level also moves over the take, between +6.7 s/d and +48.0 s/d.
  rate           717 readings of 10 s (3 left out as more than 6 σ from their 5-minute level); median 12.62, sd 17.37, reading to reading 7.24 s/d
  ...
  cycle          60.0 s (fourth wheel), 11.69 s/d p-p, explains 18% (found by the autocorrelation)
```

See [Structure in time](../series.md).

## clock

Measures and keeps each sound input's clock error, so rates can be
corrected onto true time (see [True Clock](05-readings.md#true-clock)):

```sh
timegrapher clock measure --save          # 20 minutes; nothing needs to be on the microphone
timegrapher clock list                    # the corrections kept, and where
timegrapher clock from-log --device "USB" take/clock.csv take/audio-001.wav --save
timegrapher clock set "USB" 19.3          # a known error, ppm slow
timegrapher clock forget "USB"
```

The computer's clock must be kept by internet time (NTP). Once kept, the
app, `analyze` and `long` use the correction for that input.

## report

Makes a zip to attach to a GitHub issue: an excerpt of the recording, what
was measured, the system, clock corrections and logs, with your home
folder, user name and computer name removed. It never opens the
microphone. See [Problems and Reports](14-problems-and-reports.md).

```sh
timegrapher report recording.flac --at 120 --span 300 --kind reading --note "Amplitude jumps at 2 minutes"
```

## update

Asks GitHub once whether a newer version has been published. Nothing is
downloaded or installed, and no account or identifier is sent.

```text
$ timegrapher update
Timegrapher 0.4.2 is up to date: no release has been published yet.
```

## synth

Writes a synthetic recording with a known rate, beat error and amplitude,
for trying the tools: `timegrapher synth test.wav`. `--fault-period 60`
adds a change that repeats like a wheel turning once a minute.

## For Programs and Agents

Every command takes `--json` and prints one JSON document with a `schema`
field (such as `timegrapher.analyze/2`), the software version and the input
it read. Exit codes: 0 success, 1 error. The
[agent interface](../agent-interface.md) documents every field.

---

Previous: [Sessions and Recordings](11-sessions-and-recordings.md) ·
Next: [Claude Desktop](13-claude-desktop.md)
