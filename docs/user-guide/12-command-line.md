# 12. The Command Line

The command-line tool is useful when you want a reproducible analysis,
a report from several recordings, or numerical data for your own plots.
It uses the same core engine as the desktop app. Agreement still depends
on analysing the same interval with the same settings and clock basis:
a current 10-second card reading and a whole-recording summary need not
agree on a changing watch.

The examples below use illustrative filenames. Replace them with your
own paths and use the beat rate and lift angle established for your
movement. Quote paths and calibre names containing spaces.

## Finding and Running It

The release's **Start Here** page identifies the executable. In the
current folder-based downloads, look in **Command Line**; macOS also
carries the tool inside the application bundle. From a terminal in the
tool's folder, run `./timegrapher` on Linux or macOS and
`.\timegrapher.exe` in Windows PowerShell. The Linux AppImage also accepts
`./Timegrapher.AppImage cli --help`.

The examples use `timegrapher` for readability, assuming it is on your
command search path. Otherwise use the appropriate relative or full path.
No system-wide installation or privileged symbolic link is required.

```sh
timegrapher --version
timegrapher --help
timegrapher analyze --help
```

Use a subcommand's `--help` as the authority for the installed version.
The user-facing commands are covered below; the `regress` command is
intended for engine validation rather than routine watch testing.

## Common Options

Many analysis commands accept `--bph`, `--lift`, `--notch` and `--json`,
but they do not all have identical defaults or support `--calibre`.
The most consequential distinction is:

| Command | How beat rate and lift angle are obtained |
| --- | --- |
| `analyze` | Explicit `--bph` and `--lift` override the calibre; otherwise use its known values, then detected beat rate/default 52° |
| `record` | Can use calibre values when numerical settings are absent; records context in the session |
| `session` | Explicit numerical options override the manifest; the manifest takes precedence over catalogue defaults |
| `long`, `series` | `--calibre` supplies train associations; give `--lift` separately (default 52°), and `--bph` if you need to fix the nominal rate |
| `profile`, `shape` | Supply `--lift` and, if needed, `--bph`; these commands do not take `--calibre` |

`--notch 5000,7000` removes selected steady tones in Hz where supported.
Use filtering to address an identified interference problem and record
the settings; changing filters can change the sound from which edges
are extracted.

Commands that provide `--json` return structured output suitable for
scripts. Not every command has that option: `synth`, for example, writes
a recording instead. Check help before adding a shared set of flags to
every command.

## analyze

Use `analyze` for a summary of one WAV or FLAC recording:

```sh
timegrapher analyze watch_DU.flac --calibre "ETA 2824-2"
timegrapher analyze watch_DU.flac --bph 28800 --lift 52 --json > analysis.json
timegrapher analyze watch_DU.flac --bph 28800 --lift 52 --beats beats.csv --windows windows.csv
```

The output includes rate, amplitude, unlock- and reference-based beat
error, jitter, beat coverage, clipping and other measurement conditions.
**Rate spread** describes the distribution of short-window rates over
the take, not a confidence interval for the overall rate. The CSV options
export individual beats and window measurements.

Read the clock line. `analyze` can resolve a stored input correction from
recording provenance or estimate one from suitable USB interference
lines. `--device` can select a stored input correction and `--card-ppm`
can supply an explicit value. To make a deliberate comparison on the
uncorrected sample clock, `--card-ppm 0` supplies an explicit zero correction.
Do not assume an automatically corrected CLI result should equal the
large, uncorrected desktop Rate figure.

## doctor

Use a file to inspect recorded signal quality without opening the input:

```sh
timegrapher doctor --file watch_DU.flac
```

For live setup:

```sh
timegrapher devices
timegrapher doctor --device "USB" --seconds 5
```

`devices` lists the inputs; replace USB with an identifying name from
that list. `doctor` reports signal level, clipping, beat detection and
background behaviour. On Linux it can inspect mixer controls and propose
changes. Only `--apply` applies those changes. See
[Setting Up the Microphone](03-microphone.md).

## profile

Use `profile` to inspect a particular interval and save a plot:

```sh
timegrapher profile watch_DU.flac --at 60 --span 10 --bph 28800 --lift 52 --svg profile.svg
```

Here **60–70 seconds** of the recording are selected. The output gives
both sides' beat counts, measured edge offsets, lift times and inferred
amplitudes. `--json` includes the envelope and markers. Compare the
same interval and settings with the desktop profile; see
[The Sound of the Beat](07-sound-of-the-beat.md).

## shape

`shape` measures the acoustic sound groups over a recording: rise and
peak times, intervals, levels and the share of windows in which features
were found, separately for Tick and Tock.

```sh
timegrapher shape watch_DU.flac --bph 28800 --lift 52 --json > shape.json
```

These are measurements of the recorded sound, not direct measurements
of locking depth, impulse force or lubrication. Use them to compare
repeatable changes, then inspect the waveform and movement. The
[agent interface](../agent-interface.md) documents the fields.

## calibres

```sh
timegrapher calibres --search "ETA 2824"
timegrapher calibres "ETA 2824-2"
```

The catalogue includes aliases, known numerical settings, wheel periods,
sources and your custom entries. Inspect the notes when alternatives
disagree. See [Calibres and Lift Angles](09-calibres.md).

## record

`record` adds one position to a test folder, captures its audio, checks
the take and updates `session.toml`:

```sh
timegrapher record mywatch/ --position DU --seconds 120 --calibre "ETA 2824-2" --wind-h 0
timegrapher record mywatch/ --position DD --seconds 120 --wind-h 0
timegrapher session mywatch/
```

Turn the watch and establish contact before starting each command. It
listens only for the requested duration. A newly created session uses
30 seconds of settling at the start of each take; an existing manifest
can specify a different interval. Allow enough recording time after that
settling period. The immediate quick check caps its settling exclusion
at half the take when the take is very short, so check the reported
interval rather than assuming it matches a later session report.

A take with clipping or no
regular beat is retained with those conditions reported; recording
success does not imply measurement quality.

`--wind-h` records hours since winding. Supply the actual state for each
recording rather than relying on filenames. `--file` exercises the same
workflow from an existing recording without the microphone.

## session

`session` combines positions and winding states into a report:

```sh
timegrapher session mywatch/
```

For an existing folder that has no manifest, initialise one and inspect
it before analysis:

```sh
timegrapher session --init six-positions/
```

Check each guessed position, the watch identity, settings, settling
intervals and `wind_h` values. Then run `session` on that folder. By default
it writes `report.html`, `summary.json` and `readings.csv` to
`session_report/`. The HTML is self-contained and can be opened locally
or shared as a file.

The report includes individual readings and characteristic values such
as mean rate X, positional range D, vertical-minus-horizontal DVH and,
when suitable paired data exist, Im and Ie. It also reports conditions
and detected patterns. The [session reference](../sessions.md) explains
the exact selections. A missing position changes what can be compared;
it is not a zero-valued measurement.

## long

Use `long` for a long recording or folder of segments, processed in
bounded memory. It reports timing and amplitude trends and periodic
components with possible wheel associations, and writes an HTML report.

```sh
timegrapher long overnight/ --bph 28800 --lift 52 --calibre "ETA 2824-2" --clock overnight/clock.csv --out overnight-report/
```

The explicit `--clock` log takes precedence over other clock sources.
Without a log, stored or USB-derived corrections may be available; read
the reported basis. A log must belong to the recording's audio timeline.
Omit the option if there is no suitable log.

`--calibre` supplies train periods here; it does **not** replace the
separate `--lift` setting. A reported match to a wheel's period is an
association to investigate, not proof that the wheel is faulty. See
[Long runs](../long-runs.md) and
[Steadiness](08a-steadiness.md#periods).

## series

`series` performs the analyses behind the Steadiness and Isochronism
panes:

```sh
timegrapher series overnight/ --bph 28800 --lift 52 --calibre "ETA 2824-2"
timegrapher series overnight/ --bph 28800 --lift 52 --wind-h 0 --json > series.json
timegrapher series overnight/ --bph 28800 --lift 52 --json --views > series-with-plots.json
```

The summary describes correlation, cycles, levels and trends with their
statistics. JSON also contains the isochronism bins, wind-down comparison,
rate–amplitude relationship and spectral support. `--views` adds arrays
for reconstructing the plots and increases output size.

`--reading` changes the rate-reading interval from its 10-second default;
this also changes the common interval used for the statistical summaries.
`--wind-h` supplies hours since full wind when recording began.
`--stretch-min` controls the additional distribution summaries by time
stretch. `--clock` supplies a matching clock log. This command has no
`--device` or `--card-ppm` option; otherwise it resolves correction from
recording provenance and available USB lines. Inspect the top-level
`clock` field when comparing it with the desktop panes, which use the
sample clock.

See [Steadiness](08a-steadiness.md), [Isochronism](08b-isochronism.md)
and the detailed [series reference](../series.md).

## clock

Clock calibration compares audio time with the computer's system time,
which must itself be well synchronised. No watch is needed for this
comparison.

```sh
timegrapher clock measure --save
timegrapher clock list
timegrapher clock from-log --device "USB" take/clock.csv take/audio-001.wav --save
timegrapher clock set "USB" 19.3
timegrapher clock forget "USB"
```

`measure` captures for 20 minutes by default; `--save` stores the result.
`from-log` uses an existing capture log and recording. `set` stores a
known error in **ppm slow**, negative if fast. Use it only for a value
you have established for that input. `forget` removes that input's stored
correction. The store is shared with the app.

A fixed correction is useful for offset; it cannot track changing clock
error throughout a long run. See [True Clock](05-readings.md#true-clock).

## report

`report` creates a local zip for investigating a problem:

```sh
timegrapher report watch_DU.flac --at 120 --span 300 --kind reading --note "Amplitude changes near 150 seconds"
```

The excerpt starts at 120 seconds and spans up to 300 seconds of available
audio. The bundle includes context and analysis, with common identifying
paths and names redacted. Listen to included audio and inspect the bundle
before sharing it; audio is not anonymised by removing paths. The command
does not upload the zip or open the microphone.

`report --replay BUNDLE` reruns a bundle's recorded analysis commands with
the installed build for comparison. See [Reporting a problem](../reports.md).

## update

`timegrapher update` checks GitHub for a published release and reports
whether one is newer than the installed build. It does not install it.
Use the release's instructions to update the app and accompanying tools
together.

## synth

`synth` creates test audio with specified parameters. It is useful for
learning the views without a physical watch:

```sh
timegrapher synth example.wav --duration 600 --bph 28800 --rate 5 --amplitude 270 --beat-error 0.4 --fault-period 60
timegrapher series example.wav --bph 28800 --lift 52
```

The repeating disturbance in this example is synthetic. Successful
recovery of its pattern checks an analysis path; it does not validate
all real escapement sounds or identify a real damaged tooth. `synth`
does not take `--json`.

## For Programs and Agents

JSON-producing commands include a schema identifier and software version.
Keep these with exported results, along with settings and clock context.
The [agent interface](../agent-interface.md) documents the output contracts
and command-specific errors. Normal operation uses exit code 0 for
success; runtime failures and command-line usage errors are nonzero.
`regress` also uses code 3 when validation requirements are not met.

The guide's numerical examples explain conventions. For your bench notes,
retain the actual output from your installed version and the recording
that produced it.

---

Previous: [Sessions and Recordings](11-sessions-and-recordings.md) ·
Next: [Claude Desktop](13-claude-desktop.md)
