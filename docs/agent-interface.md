# Driving the timegrapher from an AI agent

Most people who use this tool will have an AI agent to help them: to get
the microphone working, run tests and explain what the numbers mean. The
command-line tool is that agent's interface. This page is the contract.
The [watchmaker skill](../skills/watchmaker/SKILL.md) builds on it.

## Rules

- Every command has `--help`, and `timegrapher --help` lists them all.
- Every command that reports something takes `--json` and then prints
  exactly one JSON document on stdout. Progress and warnings go to stderr.
- Exit codes: `0` success, `1` error (message on stderr), `3` from `doctor`
  when the input is not fit to measure with, and from `regress` when the
  engine moved away from tg ([regress](regress.md)), and from `record` when
  the take was not fit to keep.
- Nothing listens unless asked: `doctor` and `record` open the microphone
  only while they run, for the time they say.
- Nothing changes the computer's settings unless the person asked for it:
  `doctor` proposes mixer changes and only makes them with `--apply`.
  An agent must ask the person before passing `--apply`.

## The JSON envelope

Every document starts with the same three fields:

```json
{
  "schema": "timegrapher.analyze/1",
  "software": { "name": "timegrapher", "version": "0.1.0", "git_hash": "8ec7b89", "git_dirty": false,
                "build_date": "2026-10-10", "os": "linux", "arch": "x86_64" },
  "input": {
    "files": [ { "path": "take.wav", "recording": { "source": { ... }, "mixer": { ... } } } ],
    "settings": { "bph": null, "lift_deg": 52.0, "notch_hz": [], "highpass_hz": 1500.0 }
  },
  ...the command's own fields...
}
```

`schema` is `timegrapher.<kind>/<version>`. Within a version, fields are only
added, never renamed or removed; a change that would break a reader bumps the
version. `input.files[].recording` appears when the recording has a
provenance sidecar (`take.wav.json`, written by `doctor --save`), carrying
the device, its configuration and the mixer settings at the time.

`software` tells builds apart: `git_hash` is the short commit hash
(`"unknown"` in a build from a source tarball, without git), `git_dirty`
whether the work tree had uncommitted changes (`null` when unknown), and
`build_date` the UTC date of the build; `timegrapher --version` prints the
same, e.g. `timegrapher 0.1.0 (8ec7b89, built 2026-10-10)`. These are not
readings: anything that compares two documents should leave `software` out.

| Kind | Command | The command's own fields |
|---|---|---|
| `analyze` | `analyze FILE --json` | `duration_s`, `sample_rate`, `bph`, `beats_found`, `watch_heard` (false when the beats are no steadier than noise, as on an empty stand: the median jitter is 2% of the beat period or more; every reading in the result then describes noise and must not be shown), `overall` (`rate_s_per_day`, `beat_error_ms`, `period_s`, `jitter_us`), `rate_p05`, `rate_p95`, `jitter_us`, `amplitude_deg` (+ even/odd), `amplitude_se_deg` (+ even/odd: the standard error from the spread between 2 s windows; precision, not accuracy, which rests on the lift angle), `lift_deg`, `timing_periods`, `amplitude_periods`, `clipping` (`samples`, `beats`, `beat_fraction`, `peak_dbfs`; more than 1% of beats clipped means the amplitude and beat error may be off) |
| `calibres` | `calibres [NAME] --json` | `calibres[]`: `calibre`, `maker`, `family` (other names that match), `bph`, `lift_angle_deg`, `escape_teeth`, `seconds`, `wheels[]` (`name`, `period_s` for one full turn, `teeth`, `pinion`, `basis`), `sources`, `confidence`, `notes`; a figure no source gives is `null` or left out (below) |
| `shape` | `shape FILE --json` | per pallet stone (even/odd beats): unlock, impulse and drop timing and level; see [fault-signatures.md](fault-signatures.md) |
| `profile` | `profile FILE --at S --span S --json` | `a` (the tick: even beats) and `b` (the tock: odd beats): `beats`, `t0_ms`, `step_ms`, `median`, `p10`, `p90` (envelope point by point), `floor`, `unlock_ms`, `drop_ms`, `peak_ms`, `amplitude_deg`, `sound1_ms`, `sound2_ms`, `sound3_ms`; times in ms from the beat. `--svg FILE` also draws them |
| `long` | `long FILES --json` | rate and amplitude over the run, clock calibration, periodic components named after wheels (`--calibre`, or both common escape wheels; `input.settings.named_wheels`) with false-alarm probability; also written to `summary.json` beside `report.html`; see [long-runs.md](long-runs.md) |
| `series` | `series FILES --json` | per series (`rate`, `amplitude`, `beat_error`): `verdict` (`steady`, `periodic`, `two_states`, `measurement`, `shifting_mean`, `drifting`, `wandering`, `too_short`), `headline`, and the views behind it (`autocorrelation`, `allan`, `cusum`, `changes`, `trend`, `period` (with `other_periods_s`), `two_state`, `cycle`), all three on readings of the rate's length; `isochronism` (rate by 2° amplitude bins, `s_per_day_per_deg`, `turn_deg`, `headline`); `wind_down` (first half hour against 24 h after full wind or the last half hour, `--wind-h`: `rate_change_s_per_day`, `amplitude_change_deg`, `beat_error_change_ms`); `rate_amplitude` (`r`, `p_value`, `s_per_day_per_deg`, `moves_with`, `headline`); `findings` (`code`, `series`, `severity`, `title`, `evidence`, `advice`); see [series.md](series.md) |
| `clock` | `clock list\|measure\|from-log\|set\|forget --json` | each input's stored clock error (below) |
| `devices` | `devices --json` | `devices[]`: `id`, `name`, `manufacturer`, `is_default`, `default_config`, `supported[]` (`min_channels`, `max_channels`, `min_sample_rate`, `max_sample_rate`, `sample_format`) |
| `doctor` | `doctor --json` | `source`, `check`, `mixer`, `fixes[]`, `applied`, `after`, `verdict` (below) |
| `record` | `record FOLDER --position P --json` | `take` (the take's folder), `session_file` (the session file the take was added to, or null), `position`, `wind_h`, `seconds`, `source`, `sample_rate`, `check` (`watch_heard`, `bph`, `beats_found`, `beats_expected`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms`, `clipping`; the readings are null when no watch is heard), `verdict` (`ok`, `no_watch_heard`, `clipping`); see [`record`](#record) |
| `session` | `session PATHS --json` | per-position `readings`, Witschi's characteristic values, `isochronism` per position across states of wind (`from_wind_h`, `to_wind_h`, `rate_change` s/d, `amplitude_change` deg, `beat_error_change` ms) and `findings` (`code`, `severity`, `recording`, `title`, `evidence`, `advice`); also written to `summary.json`; see [sessions.md](sessions.md) |
| `report` | `report [RECORDING] --json` | the bundle's manifest: `created_utc`, `kind`, `note`, `os_version`, `recording`, `commands[]` (`args`, `cwd`, `output`, `replayable`), `files[]` (`path`, `bytes`, `sha256`), `consent`, `redacted`, `left_out`; the zip's path goes to stdout without `--json`; see [reports.md](reports.md) |
| `replay` | `report --replay BUNDLE --json` | `bundle` (when and by which build it was made), `damaged[]`, `commands[]` (`args`, `output`, `ran`, `error`, `moved[]` of `path`, `then`, `now`) |

Tick and tock: the engine numbers beats from the first one it hears, so the tick is simply the even beats and the tock the odd ones, as tg's tic and toc are; it cannot tell which pallet stone is which. Fields named `even`/`odd` or `a`/`b` mean tick/tock in that order, and people-facing text says Tick and Tock.

Rates are seconds per day, positive gaining. Rates from a recording are on
the sound card's clock unless `long` was given a clock log or a stored
correction; a sound card can be tens of ppm off (1 ppm is 0.0864 s/d).

## Calibres

The lift angle sets the amplitude, so take it from the calibre, never
from memory. `timegrapher calibres` lists the built-in table (the
crate's `data/train-wheels.json`, whose README says where each figure
comes from); `calibres NAME` shows one, matched on its name, its name
without the maker, or one of its `family` names, ignoring case, spaces
and punctuation. An unknown name exits 1.

`analyze --calibre NAME` sets the beat rate, the lift angle and the
escape wheel teeth from the table (52° and 15 teeth where it gives
none) and names `timing_periods` and `amplitude_periods` after that
calibre's wheels. `--bph`, `--lift` and
`--escape-teeth` still win over it. Without a calibre, `analyze` guesses
the beat rate and uses 52° and a 15-tooth escape wheel. The calibre and
the settings used are in `input.settings` (`calibre`, `bph`, `lift_deg`,
`escape_teeth`). A watch whose rate differs from its calibre's (a
modified movement) needs `--bph`.

## `clock`

```sh
timegrapher clock list
timegrapher clock measure [--device NAME] [--minutes 20] [--save]
timegrapher clock from-log LOG RECORDING --device NAME [--save]
timegrapher clock set NAME PPM
timegrapher clock forget NAME
```

Keeps each input's clock error in `clocks.json` (schema
`timegrapher.clocks/1`) in the settings folder: `$TIMEGRAPHER_CONFIG_DIR`,
else `~/.config/timegrapher` on Linux, `~/Library/Application
Support/timegrapher` on macOS, `%APPDATA%\timegrapher` on Windows. Each
entry is `device`, `ppm` (positive: the card is slow, so uncorrected rates
read fast by `ppm` × 0.0864 s/d), `ppm_sd`, `span_s`, `measured_utc` and
`source` (`measure`, `log` or `set`). `measure` listens (nothing need be on
the microphone) and fits the audio that arrives against the system clock,
which must be kept by NTP; 20 minutes gives about ±0.3 ppm. `from-log`
takes the error from a recording's clock log. Neither stores anything
without `--save`.

`analyze` adds the corrected rate beside the card's: with `--card-ppm N`,
`--device NAME`, or a stored entry for the input the recording names, its
document gains `clock` (`ppm`, `from`, `rate_error_s_per_day`,
`rate_s_per_day`). Every other field, `overall.rate_s_per_day` included,
stays on the card's clock, as tg reads it, so comparisons with tg and the
regression baseline are unchanged. `long` applies the correction to its
whole report (see [long-runs.md](long-runs.md)).

## `record`

```sh
timegrapher record FOLDER --position DU [--seconds 120] [--device NAME]
                   [--watch TEXT] [--calibre NAME] [--bph N] [--lift DEG]
                   [--wind-h H] [--file AUDIO] [--json]
```

Records one position of a test, for `--seconds` (default 120), and stops.
It listens only while it runs, says on stderr what it is recording and
for how long, and prints `done: the watch can be moved` when the watch may
be turned. An agent runs it once per position, after telling the person
which way to turn the watch, and never on its own initiative.

Each run makes one take folder in FOLDER, the same as the desktop app
saves (`audio-001.wav`, `clock.csv`, `session.json`, and a `session.toml`
the app opens it with), named after the start time, the watch and the
position. The take is analysed at once. When a watch is heard and under
1% of beats clip, the take is added to `FOLDER/session.toml` (written on
the first take with the watch, calibre, bph, lift and `settle_s = 30`,
then appended to, comments and all) with its position, `wind_h` and start
time, and the exit code is 0. Otherwise the take folder is kept, the
session file is left alone, `verdict` says why, and the exit code is 3:
record that position again. `timegrapher session FOLDER` then reads every
take in the session.

`--calibre` fills bph and lift from the calibre table when they are not
given. A session file already in FOLDER sets them for every take; a
`--bph` or `--lift` that disagrees with it is an error, so one session is
measured one way. `--wind-h` is the hours since full wind (0 for the
fully wound half of a test, 24 for the run-down half). `--file` reads a
recording instead of the microphone, at once rather than in real time,
to try the command.

## `doctor`

```sh
timegrapher doctor [--device NAME] [--card N] [--seconds 5] [--file WAV]
                   [--save WAV] [--apply] [--bph N] [--json]
```

Records a few seconds from the default input (or `--device`, matched against
the id or name from `devices`), or reads `--file`, and reports in `check`:

| Field | Meaning |
|---|---|
| `peak_dbfs`, `rms_dbfs` | level relative to full scale |
| `clipped_samples` | samples at full scale, plus flat tops below it (clipping before the converter) |
| `bph`, `beats_expected`, `beats_found` | is a steady beat heard? |
| `tick_level_dbfs`, `noise_level_dbfs`, `tick_to_noise_db` | ticks against the background between them (high-passed at 1.5 kHz) |
| `signal_x`, `signal_band` | median beat peak over the median envelope, the desktop app's "signal N×", and its band (below) |
| `gap_rise_db` | background late in the gap over early in it; several dB means automatic gain is pumping |
| `rate_s_per_day`, `beat_error_ms` | a quick look only |
| `suggested_gain_change_db` | what would bring the ticks to about −10 dBFS (−6 dB step when clipped) |
| `issues[]` | `code`, `severity` (`fault` spoils measurements, `warning` does not), `title`, `evidence`, `advice`: the same shape as a test session's findings |

Issue codes: `silent`, `too_quiet`, `clipping`, `hot`, `agc_suspected`,
`no_ticks`, `noisy`.

`signal_band` is `good` (10× and above: all readings can be trusted),
`fair` (5 to 10×: rate reliable, amplitude and beat error may be off),
`poor` (3 to 5×: only the rate) or `none` (below 3×: no watch heard;
pure noise reads about 2×). These are the desktop app's bands and its
"signal N×" figure, so an agent and the app's status bar agree. `no_ticks` is raised when the band is `none` (or
fewer than half the beats are found), `noisy` as a fault when it is `poor`
and as a warning when it is `fair`.

On Linux, `mixer` holds the sound card's ALSA controls (read with
`amixer -c CARD scontents`; the card comes from `--card`, the device id, or
the only USB sound card), and each entry in `fixes` has the exact `amixer`
command. Automatic gain is turned off first and alone, because levels
measured while it acts mean little. On macOS and Windows the fixes say
which setting to change. Every fix has `requires_consent: true`.

With `--apply` the commands run and, when listening live, the check runs
again and appears as `after`. `verdict` is `ok` or `needs_attention`, and
the exit code follows it.

Example (illustrative): the C-Media USB timegrapher microphone arrives with automatic gain
on and the level at 16/16, and clips every tick:

```
$ timegrapher doctor
Level        peak 0.0 dBFS, RMS -10.7 dBFS, 10731 clipped samples
...
FAULT        The input clips (10731 samples clipped, peak 0.0 dBFS). Turn the input level down ...
Mixer        card 2: 'Mic' 16/16 (+23.8 dB), 'Auto Gain Control' on
Suggested changes:
  - Turn off 'Auto Gain Control' on card 2 so the gain stays fixed, then run doctor again.
      amixer -c 2 sset 'Auto Gain Control' off
Nothing was changed. Run again with --apply to make these changes.
```

## Building without sound input

Sound input uses `cpal` behind the default `live` feature (on Linux it needs
the ALSA development package, `libasound2-dev`). `cargo build
--no-default-features` builds without it; `doctor --file` still works.

## Later

- `schema KIND`: print a JSON Schema for each document.
- An MCP server, if live streaming to an agent or use from chat-only apps
  turns out to need it; it would wrap the same core.
