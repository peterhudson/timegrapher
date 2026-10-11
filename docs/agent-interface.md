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
| `analyze` | `analyze FILE --json` | `duration_s`, `sample_rate`, `bph`, `beats_found`, `watch_heard` (false when the beats are no steadier than noise, as on an empty stand: the median jitter is 2% of the beat period or more; every reading in the result then describes noise and must not be shown), `overall` (`rate_s_per_day`, `beat_error_ms`, `period_s`, `jitter_us`), `rate_p05`, `rate_p95`, `jitter_us`, `amplitude_deg` (+ even/odd), `amplitude_se_deg` (+ even/odd: the standard error from the spread between 2 s windows; precision, not accuracy, which rests on the lift angle), `lift_deg`, `timing_periods`, `amplitude_periods`, `clipping` (`samples`, `beats`, `beat_fraction`, `peak_dbfs`; more than 1% of beats clipped means the amplitude and beat error may be off), `usb_clock` (`ppm` the sound card runs slow, `lines`, `spread_ppm`: read from the USB 1 kHz lines in the recording, `null` when it has none), `rate_corrected_s_per_day` (the whole-recording rate corrected with `usb_clock`; `overall.rate_s_per_day` stays on the card's clock, as tg reads it), `doubts` (`clipped`, `beats_missing`, `lift_unknown`, `clock_unknown`: each lowers the grades it touches by one step), `confidence` (`rate`, `amplitude`, `beat_error`, each `se` in the reading's unit and `grade` `good`, `fair` or `poor`; `null` when no watch is heard). Good means a ± within 0.5 s/d, 2° or 0.05 ms; fair within 2 s/d, 5° or 0.1 ms. Fewer than 10 s of beats, or a set beat rate the recording disagrees with, grades every reading poor (the `short_span` and `set_rate_disagrees` caveats), so `confidence` keeps its shape. `faults[]`: possible escapement faults from the shape of the Tick and Tock, each `id` (`unlock_too_strong`, `no_unlock_sound`, `escapement_friction`, `unlock_uneven`, `rubbing_after_drop`, `beat_error_drift`), `label` (the fault in a watchmaker's words), `side` (`tick`, `tock` or `both`), `strength` (`possible`, or `likely` once a rule has been confirmed on a real fault; every rule is `possible` for now), `evidence` (the measures behind it, by name), `see` (where the fault is described). Empty when nothing is seen, and also when the shape cannot be judged: no watch heard, clipping, or amplitude or beat error too unsteady to grade fair. A finding is worth checking on the bench, not a diagnosis. A `clock` block (`ppm`, `from`, `rate_s_per_day`) appears when a correction is stored, given or read from the USB lines, in that order of preference. When `watch_heard` is false every reading is `null` (`overall`, the amplitudes, `beat_error_unlock_ms`, `rate_p05`/`rate_p95`, `rate_corrected_s_per_day`, `confidence`) and `timing_periods`, `amplitude_periods` and `faults` are empty: no watch was heard. `bph_heard` appears when a set beat rate (`--bph` or `--calibre`) differs from the rate the recording sounds like; every grade is then `poor`. `beat_error_unlock_ms` is the signed beat error measured from the unlock (the median over the 2 s amplitude windows), the figure checked against tg's beat error in regress; `overall.beat_error_ms` is the drop's. Sign, for both: positive means Tick comes late, so the half-beat from Tock to Tick is longer than the one from Tick to Tock, by twice the value. Tick is the first beat of the run, so the sign holds within one run but not across runs or between `analyze` and a live reading of the same take; show the size, and compare signs only within one run. `caveats[]`: what limits how far the readings can be trusted, each with a `code`: `no_watch` (readings withheld), `set_rate_disagrees` (`heard`; every grade poor), `short_span` (`span_s`, under 10 s of beats; every grade poor), `clipped` (`fraction`; amplitude and beat error one step down), `beats_missing` (`fraction` found; all one step down), `lift_unknown` (amplitude one step down), `clock_unknown` (rate one step down), `breaks` (`count`). Empty when nothing limits them. `analyze` and `session` share one beat pipeline (the chunked stream that `long`, `series` and `regress` read too), so the same file gives the same beats and readings in both. |
| `calibres` | `calibres [NAME] --json` | `calibres[]`: `calibre`, `maker`, `family` (other names that match), `bph`, `lift_angle_deg`, `escape_teeth`, `seconds`, `wheels[]` (`name`, `period_s` for one full turn, `teeth`, `pinion`, `basis`), `sources`, `confidence`, `notes`; a figure no source gives is `null` or left out (below) |
| `shape` | `shape FILE --json` | per pallet stone (even/odd beats): unlock, impulse and drop timing and level; see [fault-signatures.md](fault-signatures.md) |
| `profile` | `profile FILE --at S --span S --json` | `a` (the tick: even beats) and `b` (the tock: odd beats): `beats`, `t0_ms`, `step_ms`, `median`, `p10`, `p90` (envelope point by point), `floor`, `unlock_ms`, `drop_ms`, `peak_ms`, `amplitude_deg`, `sound1_ms`, `sound2_ms`, `sound3_ms`; times in ms from the beat. `--svg FILE` also draws them |
| `long` | `long FILES --json` | rate and amplitude over the run, clock calibration, periodic components named after wheels (`--calibre`, or both common escape wheels; `input.settings.named_wheels`) with false-alarm probability; also written to `summary.json` beside `report.html`; see [long-runs.md](long-runs.md) Without a clock log, the clock is corrected as `analyze` corrects it: a stored or given error, else the recording's USB lines. tg and a session's `reference` rates are on the sound card's clock. `breaks` (`audio_s`, `gap_s`; present only for a take that was paused or resumed): times run on with each gap in, rate and beat error are fitted on each side of a break apart, and no window or slice spans one. `clock.breaks` lists the pauses the clock log shows. |
| `series` | `series FILES --json` | per series (`rate`, `amplitude`, `beat_error`): `verdict` (`steady`, `periodic`, `two_states`, `measurement`, `shifting_mean`, `drifting`, `wandering`, `too_short`), `headline`, and the views behind it (`autocorrelation`, `allan`, `cusum`, `changes`, `trend`, `period` (with `other_periods_s`), `two_state`, `cycle`), all three on readings of the rate's length; `isochronism` (rate by 2° amplitude bins, `s_per_day_per_deg`, `turn_deg`, `headline`); `wind_down` (first half hour against 24 h after full wind or the last half hour, `--wind-h`: `rate_change_s_per_day`, `amplitude_change_deg`, `beat_error_change_ms`); `rate_amplitude` (`r`, `p_value`, `s_per_day_per_deg`, `moves_with`, `headline`); `distributions` (the app's histograms over the take and each `--stretch-min` stretch: `series`, `values` (`readings` or `2 s`), `edges`, `counts`, `below`, `above`, `median`, `p10`, `p90`, `peaks`, `normal_line`, `quantiles`, `summary`); `findings` (`code`, `series`, `severity`, `title`, `evidence`, `advice`); with `--views`, the chart data as well (each series' `views`: `readings`, `search`, `fold`; `period_span`; the whole take's distributions' `points`, `[value, z]`); see [series.md](series.md) Without a clock log, the clock is corrected as `analyze` corrects it: a stored or given error, else the recording's USB lines. tg and a session's `reference` rates are on the sound card's clock. `breaks` (`audio_s`, `gap_s`; present only for a take that was paused or resumed): times run on with each gap in, rate and beat error are fitted on each side of a break apart, and no window or slice spans one. |
| `clock` | `clock list\|measure\|from-log\|set\|forget --json` | each input's stored clock error (below) |
| `devices` | `devices --json` | `devices[]`: `id`, `name`, `manufacturer`, `is_default`, `default_config`, `supported[]` (`min_channels`, `max_channels`, `min_sample_rate`, `max_sample_rate`, `sample_format`) |
| `doctor` | `doctor --json` | `source`, `check`, `mixer`, `fixes[]`, `applied`, `after`, `verdict` (below) |
| `record` | `record FOLDER --position P --json` | `take` (the take's folder), `session_file` (the session file the take was added to, or null), `position`, `wind_h` (null when not given), `seconds`, `source`, `sample_rate`, `check` (`from_s`, `to_s`: the stretch read, after the settling time; `watch_heard`, `bph`, `beats_found`, `beats_expected`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms` (signed, from the unlock), `clipping`; `watch_heard` and `clipping` cover the whole take; the readings are those `session` gives for the take, null when no watch is heard or the stretch after settling is too short), `verdict` (`ok`, `no_watch_heard`, `clipping`); see [`record`](#record) |
| `session` | `session PATHS --json` | `input.settings.calibre` (the table calibre used, from `--calibre` or the session file's `calibre` line) and `calibre_notes` (where explicit values disagree with it), per-position `readings`, Witschi's characteristic values, `isochronism` per position across states of wind (`from_wind_h`, `to_wind_h`, `rate_change` s/d, `amplitude_change` deg, `beat_error_change` ms) and `findings` (`code`, `severity`, `recording`, `title`, `evidence`, `advice`); also written to `summary.json`; see [sessions.md](sessions.md) Each reading's measurement has `breaks` too, with the same meaning, when its stretch holds one. Each reading's `measurement` also carries `confidence` (`rate`, `amplitude`, `beat_error`, each `se` and `grade`, judged as `analyze` judges them; `null` when no watch is heard) and `caveats[]` (as for `analyze`; `lift_unknown` when no `--lift`, session-file `lift` or known calibre gave the lift angle). |
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
                   [--wind-h H] [--file AUDIO] [--live PATH] [--json]
```

Records one position of a test, for `--seconds` (default 120), and stops.
It listens only while it runs, says on stderr what it is recording and
for how long, and prints `done: the watch can be moved` when the watch may
be turned. An agent runs it once per position, after telling the person
which way to turn the watch, and never on its own initiative.

Each run makes one take folder in FOLDER, the same as the desktop app
saves (`audio-001.wav`, `clock.csv`, `session.json`, and a `session.toml`
the app opens it with; a take paused and resumed in the app also has a
`breaks` list in `session.json`, described in [app.md](app.md)), named after the start time, the watch and the
position. The take is analysed at once. Whether a watch is heard and clipping
are judged over the whole take, so a short take still gets a verdict.
The readings come from the stretch `session` will read: the check skips
the take's `settle_s` from the session file (30 s in a file `record`
writes; at most half the take) and reads the rest on the sound card's
clock, as `session` does, so the check and the report agree; they are
null when that stretch is too short to hear the watch on its own. When a
watch is heard and under
1% of beats clip, the take is added to `FOLDER/session.toml` (written on
the first take with the watch, calibre, bph, lift and `settle_s = 30`,
then appended to, comments and all) with its position, `wind_h` when
given, and start time, and the exit code is 0. Otherwise the take folder is kept, the
session file is left alone, `verdict` says why, and the exit code is 3:
record that position again. `timegrapher session FOLDER` then reads every
take in the session.

`--calibre` fills bph and lift from the calibre table when they are not
given. A session file already in FOLDER sets them for every take; a
`--bph` or `--lift` that disagrees with it is an error, so one session is
measured one way. `--wind-h` is the hours since full wind (0 for the
fully wound half of a test, 24 for the run-down half); without it the
take carries no `wind_h`, and the report says the state of wind was not
given. `--file` reads a
recording instead of the microphone, at once rather than in real time,
to try the command.

`--live PATH` keeps the take's latest readings in PATH while it records,
for a live view (the MCP server's Live view reads it). The file is
replaced whole about once a second of audio, through `PATH` with a `.tmp`
extension and a rename, so a reader never sees half of it. The last one
says `done: true`. With `--file`, the recording is played back in real
time, as it would have been heard. The take, the session file, the exit
code and the `--json` document on stdout are the same with or without it.
The file holds one `timegrapher.live/1` document:

| Field | What |
|---|---|
| `position`, `seconds` | The take's position and length |
| `recorded_s` | Seconds recorded so far |
| `done` | The recording has ended (the take is still being checked) |
| `average_s` | The averaging time of `reading`, 10 s as in the app |
| `beats_heard` | Beats heard so far |
| `reading` | The live reading as the app shows it: `rate_s_per_day`, `amplitude_deg` (`amplitude_error_deg`), `beat_error_unlock_ms` (the headline) and `beat_error_ms` (from the drop), `bph`, `beats_used`, `span_s`, `jitter_us`, `snr`, `clipped_beats`, `clipped_fraction`, `watch_heard`; readings are null until enough beats are heard, and whenever `watch_heard` is false (beats scattered like noise, as `analyze` nulls them) |
| `strip` | The paper strip: `span_s` (30), `period_s`, and `beats` as `[time_s, lead_ms, side]` for the last 30 s. `lead_ms` is how early the beat came against a clock at the nominal rate, anchored on the first 16 beats as the app anchors its strip. `side` is 0 for the Tick and 1 for the Tock |
| `trend` | The readings about once a second as columns `t_s`, `rate`, `amplitude`, `beat_error` (from the unlock). It keeps the whole take, up to 300 points: past that, every other point of the older half is dropped |

The file stays bounded: at most 300 trend points, and 30 s of beats
(300 at 36,000 bph).

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

## `update`

```sh
timegrapher update [--json]
```

Asks GitHub once whether a newer release is published (schema
`timegrapher.update/1`; `input.source` is the API address). The body is
`current` (this program's version), `latest` (`version` without the "v",
`url` of the release page, `published_at`, `notes` in Markdown) and
`update_available`. While no release has been published, `latest` is
null and `update_available` false, and it exits 0. It sends no account or
identifier, downloads and installs nothing, and never offers drafts or
pre-releases. When GitHub cannot be reached or is limiting requests, it
exits 1 with the reason on stderr. A person updates by downloading the new release
from `latest.url` ([install.md](install.md)). Builds without the default
`update-check` feature have no network code and say so.

## Building without sound input

Sound input uses `cpal` behind the default `live` feature (on Linux it needs
the ALSA development package, `libasound2-dev`). `cargo build
--no-default-features` builds without it, and without `update`; `doctor
--file` still works.

## Later

- `schema KIND`: print a JSON Schema for each document.
- An MCP server, if live streaming to an agent or use from chat-only apps
  turns out to need it; it would wrap the same core.
