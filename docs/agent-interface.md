# Driving the timegrapher from an AI agent

Most people who use this tool will have an AI agent to help them: to get
the microphone working, run tests and explain what the numbers mean. The
command-line tool is that agent's interface. This page is the contract.
The [watchmaker skill](../skills/watchmaker/SKILL.md) builds on it.

## Rules

- Every command has `--help`, and `timegrapher --help` lists them all.
- Every command that reports something takes `--json` and then prints
  exactly one JSON document on stdout. Progress and warnings go to stderr.
- Exit codes: `0` success, `1` error (message on stderr), `3` from
  `regress` when the engine moved away from tg ([regress](regress.md)).
  `record` exits 0 for every take it keeps, which is every take. `doctor`
  exits `0` whatever it detects.
- Nothing listens unless asked: `doctor` and `record` open the microphone
  only while they run, for the time they say.
- Nothing changes the computer's settings unless the person asked for it:
  `doctor` lists mixer changes and only makes them with `--apply`.
  An agent must ask the person before passing `--apply`.

## The JSON envelope

Every document starts with the same three fields:

```json
{
  "schema": "timegrapher.analyze/2",
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
version. `analyze`, `doctor`, `record`, `series`, `session` and
`regress-holdout` are at version 2: it removed their grades, verdicts,
named faults and advice (the software reports what it measured and
detected; the [watchmaker skill](../skills/watchmaker/SKILL.md) interprets
it), and `record/2` keeps every take. Other kinds are at version 1. `input.files[].recording` appears when the recording has a
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
| `analyze` | `analyze FILE --json` | Schema `timegrapher.analyze/2`. `duration_s`, `sample_rate`, `bph`, `beats_found`, `beats_found_fraction` (`beats_found` over the beats the recording's length holds at `bph`), `watch_heard` (false when a regular beat was looked for and not found: with 10 s of sound or more, the timing scatter `jitter_us` is 2% of the beat period or more, as on an empty stand; with less than 10 s, regularity is not assessed and `watch_heard` is true), `overall` (`rate_s_per_day`, `beat_error_ms`, `period_s`, `jitter_us`), `rate_p05`, `rate_p95`, `jitter_us` (the timing scatter, median over the 10 s windows, µs), `timing_scatter_fraction` (`jitter_us` as a share of the beat period, 0 to 1; `watch_heard` is false when it is 0.02 or more), `amplitude_deg` (+ even/odd), `amplitude_se_deg` (+ even/odd: the standard error from the spread between 2 s windows; precision, not accuracy, which rests on the lift angle), `lift_deg`, `timing_periods`, `amplitude_periods`, `clipping` (`samples`, `beats`, `beat_fraction`, `peak_dbfs`), `usb_clock` (`ppm` the sound card runs slow, `lines`, `spread_ppm`: read from the USB 1 kHz lines in the recording, `null` when it has none), `rate_s_per_day` (the whole-recording rate on the card's clock, as tg reads it; the same figure as `overall.rate_s_per_day`), `rate_corrected_s_per_day` (that rate corrected with `usb_clock`), `doubts` (`clipped`, `beats_missing`, `lift_unknown`, `clock_unknown`: conditions detected, as flags; each one set is also in `caveats`), `confidence` (`rate`, `amplitude`, `beat_error`, each `se`, the standard error in the reading's unit; `null` when no regular beat is found), `shape` (`even` and `odd`, the Tick and the Tock, each `windows_measured`, `share_with_sound_1`, `share_with_sound_2`, `i12_ms`, `i13_ms`, `ratio13`, `tail_ratio`; and `beat_error_first_quarter_ms`, `beat_error_last_quarter_ms`, `beat_error_quarter_windows`: the unlock beat error early and late in the recording; see [fault-signatures.md](fault-signatures.md); `null` when no regular beat is found). A `clock` block (`ppm`, `from`, `rate_s_per_day`) appears when a correction is stored, given or read from the USB lines, in that order of preference. When `watch_heard` is false every reading is `null` (`overall`, the amplitudes, `beat_error_unlock_ms`, `rate_p05`/`rate_p95`, `rate_s_per_day`, `rate_corrected_s_per_day`, `confidence`, `shape`) and `timing_periods` and `amplitude_periods` are empty; `jitter_us`, `timing_scatter_fraction`, `bph` and `beats_found` stay (the rate listened for and the sounds found, not a watch's readings). `bph_heard` appears when a set beat rate (`--bph` or `--calibre`) differs from the rate the recording sounds like. `beat_error_unlock_ms` is the signed beat error measured from the unlock (the median over the 2 s amplitude windows), the figure checked against tg's beat error in regress; `overall.beat_error_ms` is the drop's. Sign, for both: positive means Tick comes late, so the half-beat from Tock to Tick is longer than the one from Tick to Tock, by twice the value. Tick is the first beat of the run, so the sign holds within one run but not across runs or between `analyze` and a live reading of the same take; show the size, and compare signs only within one run. Over a stretch with holes, each side of a hole is measured on its own and the beat errors combine as their beat-weighted mean (`overall.beat_error_ms`) or the median over all their 2 s windows (`beat_error_unlock_ms`); where the sign turns over across a hole (as the drop's fit finds it: only a stretch whose beats span 10 s or more is fitted, and the unlock follows the same decision), they combine as sizes and are given as positive, so the turn does not cancel the size the watch has either side, and the caveat `beat_error_as_size` says so. A `session` measurement's two beat errors combine the same way, and so do `long`'s; where they do, `series`' beat error readings are each given as a size, and both carry the same caveat. Where the sound has a hole in it (listening stopped and started again), the sides keep their names across it by their sound (where they are not told apart by their sound, by the beat count across it, and the caveat `sides_by_count` says so), the hole counts in `breaks`, and the rate is fitted on each side of it apart. `caveats[]`: conditions detected, each with a `code`: `no_watch` (no regular beat found; readings withheld), `set_rate_disagrees` (`heard`), `short_span` (`span_s`, the beats span under 10 s), `clipped` (`fraction`, more than 1% of beats clipped), `beats_missing` (`fraction` found, under 95%), `lift_unknown` (the lift angle is the default), `clock_unknown` (the sound card's clock is not corrected), `breaks` (`count`: holes in the sound, where the beats jump), `sides_by_count` (`count`: of those, the ones where Tick and Tock were not told apart by their sound, so the sides after each are named by the beat count across it), `masked` (`seconds`: stretches masked out of the readings by hand, as a session file's `masked` or the app's Paper Strip leaves out), `beat_error_as_size` (the beat error's sign differs either side of a cut, so `overall.beat_error_ms` and `beat_error_unlock_ms` are sizes, positive, not signed). None of them changes a reading or its ±. Empty when none is detected. Version 2 removed `confidence.*.grade`, `faults[]` and every grading rule. `analyze` and `session` share one beat pipeline (the chunked stream that `long`, `series` and `regress` read too), so the same file gives the same beats and readings in both. |
| `calibres` | `calibres [NAME \| --search QUERY] --json` | `calibres[]` (the shipped table): `calibre`, `maker`, `family` (other names that match), `former_names` (names the row had in earlier versions of the table, which still match; left out when there are none), `common`, `bph`, `lift_angle_deg`, `escape_teeth`, `seconds`, `wheels[]` (`name`, `period_s` for one full turn, `teeth`, `pinion`, `basis`), `sources`, `confidence`, `notes`; `custom[]` (calibres the user added: `name`, `maker`, `aliases`, `bph`, `lift_angle_deg`, `escape_teeth`, `wheels[]`, `notes`); with NAME, `slots[]` (`name`, `period_s`, `origin`: `shipped`, `user` or `unknown`, `shipped_period_s` when the user's replaces the table's, `basis`); `listed` (`common` with neither NAME nor `--search`, which lists the common calibres and the user's; else `matching`), `table_size` (calibres in the shipped table); `input.settings.user_calibres` is the file read; a figure no source gives is `null` or left out (below) |
| `shape` | `shape FILE --json` | per pallet stone (even/odd beats): unlock, impulse and drop timing and level; see [fault-signatures.md](fault-signatures.md) |
| `profile` | `profile FILE --at S --span S --json` | `a` (the tick: even beats) and `b` (the tock: odd beats): `beats`, `t0_ms`, `step_ms`, `median`, `p10`, `p90` (envelope point by point), `floor`, `unlock_ms`, `drop_ms`, `peak_ms`, `amplitude_deg`, `sound1_ms`, `sound2_ms`, `sound3_ms`; times in ms from the beat. `--svg FILE` also draws them |
| `long` | `long FILES --json` | rate and amplitude over the run, clock calibration, periodic components named after wheels (`--calibre`, or both common escape wheels; `input.settings.named_wheels`) with false-alarm probability; also written to `summary.json` beside `report.html`; see [long-runs.md](long-runs.md) Without a clock log, the clock is corrected as `analyze` corrects it: a stored or given error, else the recording's USB lines. tg and a session's `reference` rates are on the sound card's clock. `breaks` (`audio_s`, `gap_s`; present only for a take that was paused or resumed): times run on with each gap in, rate and beat error are fitted on each side of a break apart (the beat error combined as for `analyze`, and where it combines as sizes the slices' beat errors are sizes too), and no window or slice spans one. `clock.breaks` lists the pauses the clock log shows. `caveats[]`: `sides_by_count` (`count`) and `beat_error_as_size` (the take's beat errors, its slices' and `series`' beat error readings are sizes, positive, not signed) as for `analyze`; empty when none. |
| `series` | `series FILES --json` | schema `timegrapher.series/2` (version 2 removed `verdict`, the findings' `severity` and `advice`, and `moves_with`); `clock` (`ppm`, `rate_error_s_per_day`, `from`, `tracks_drift`) describes the fit actually applied, or is `null` when uncorrected. Positive ppm means the card runs slow; for a drifting log, ppm summarizes the fit. No calibration uncertainty is claimed. An explicit `--clock` log takes precedence, otherwise the first recording's stored device correction (including zero), then its USB lines; `series` has no `--card-ppm` or `--device` options. Per series (`rate`, `amplitude`, `beat_error`): `headline` (what the tests detected, stated as facts with their statistics), `unlock_hopping`, and the tests behind it (`autocorrelation`, `allan`, `cusum`, `changes`, `trend`, `period` (with `other_periods_s`), `two_state`, `cycle`), all three on readings of the rate's length; `isochronism` (rate by 2° amplitude bins, `s_per_day_per_deg`, `turn_deg`, `headline`); `wind_down` (first half hour against 24 h after full wind or the last half hour, `--wind-h`: `rate_change_s_per_day`, `amplitude_change_deg`, `beat_error_change_ms`); `rate_amplitude` (`r`, `p_value`, `effective_readings`, `s_per_day_per_deg`, `take_r`, `take_s_per_day_per_deg`, `headline`; always given, with no threshold); `distributions` (the app's histograms over the take and each `--stretch-min` stretch: `series`, `values` (`readings` or `2 s`), `edges`, `counts`, `below`, `above`, `median`, `p10`, `p90`, `peaks`, `normal_line`, `quantiles`, `summary`); `findings` (`code`, `series`, `title`, `evidence`: one per series where structure was detected); `spectra` (per series: `series`, `unit`, `step_s`, `period_s[]` longest first, `amplitude[]`: the plain Hann-windowed amplitude spectrum, at most 600 points on a log period axis; `support` gives finite/total FFT input-grid samples before padding, not acoustic coverage; missing samples are zero-filled after mean removal without missing-data renormalization); with `--views`, the chart data as well (each series' `views`: `readings`, `search`, `fold`; `period_span`; the whole take's distributions' `points`, `[value, z]`); with `--maps`, `time_frequency` (an STFT and a synchrosqueezed wavelet transform map of each series: `series`, `method`, `unit`, `step_s`, `window_s`, `hop_s`, `period_s[]`, `t_s[]`, `values[][]`, null where nothing was read) and `coherence` (the wavelet coherence of the rate with the amplitude and of Tick's amplitude with Tock's: `x`, `y`, `step_s`, `period_s[]`, `t_s[]`, `coherence[][]`, `phase_deg[][]`, `noise_95[]`, `noise_pairs`) and `lomb_scargle` (each series' readings' periodogram, gaps and all: `series`, `unit`, `readings`, `span_s`, `period_s[]`, `power[]`, `window[]`, `peaks[]` with `period_s`, `power`, `false_alarm`, `window_power`, `on_window_line`, `wheel`) and `wheel_turns` (each named wheel's turn averaged over the take and per 20° of amplitude: `profile[]`, `se[]`, `turns`); `wheel_lines` (each named wheel's line size in the beat timing, amplitude and beat error, by 10° of amplitude: `bins[]` with `from_deg`, `windows`, `size`, `se`, and for the rate `size_s_per_day`, `se_s_per_day`; every window with `--maps`) is in the plain JSON too; see [series.md](series.md) tg reference rates remain on the sound card's clock; session `reference` values are supplied by the caller and their clock basis is not recorded. `breaks` (`audio_s`, `gap_s`; present only for a take that was paused or resumed): times run on with each gap in, rate and beat error are fitted on each side of a break apart, and no window or slice spans one; where the beat error's sign turns over across a break (as for `analyze`), the beat error series is read as sizes, given as positive. `caveats[]`: `sides_by_count` (`count`) and `beat_error_as_size` (the `beat_error` series' readings, and so its `median` and the tests on it, are sizes, positive, not signed) as for `analyze`; empty when none. |
| `clock` | `clock list\|measure\|from-log\|set\|forget --json` | each input's stored clock error (below) |
| `watches` | `watches list [--all] [--library DIR] --json` | the app's library of watches, read only: `library`, `bytes`, `watches[]` (`name`, `calibre`, `bph`, `lift_deg`, `archived`, `takes` (how many), `bytes`, `newest_started_utc`), `unsorted` (how many takes Quick Listen made are not yet filed under a watch), `unreadable[]`, `caveats[]` (none yet) (see [`watches`](#watches)) |
| `watch` | `watches show NAME [--library DIR] --json` | one watch, or `Unsorted`: `library`, `name`, `unsorted`, `folder`, `calibre`, `bph`, `lift_deg`, `notes`, `archived`, `fields[]` (`label`, `kind`, `value`), `photos` (how many), `bytes`, `takes[]` newest first (`path`, `name`, `started_utc`, `duration_s`, `positions[]`, `audio`, `incomplete`, `bytes`, `at_end`), `unreadable[]`, `caveats[]` (none yet) |
| `devices` | `devices --json` | `devices[]`: `id`, `name`, `manufacturer`, `is_default`, `default_config`, `supported[]` (`min_channels`, `max_channels`, `min_sample_rate`, `max_sample_rate`, `sample_format`) |
| `doctor` | `doctor --json` | Schema `timegrapher.doctor/2`. `source`, `check`, `mixer`, `fixes[]`, `applied`, `after` (below). Version 2 removed `verdict`, `check.signal_band`, `issues[].severity`, `issues[].advice` and the `noisy` issue. |
| `record` | `record FOLDER --position P --json` | (`record/2`) `take` (the take's folder), `library_watch` (the library watch the session folder was made under, without a FOLDER; else null), `session_file` (the session file the take was added to), `position`, `wind_h` (null when not given), `seconds`, `source`, `sample_rate`, `check` (`from_s`, `to_s`: the stretch read, after the settling time; `watch_heard`, `timing_scatter_ms` and `timing_scatter_fraction` (the median timing scatter over the whole take, in ms and as a share of the beat; 2% or more is no regular beat), `bph`, `beats_found`, `beats_expected`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms` (signed, from the unlock), `clipping`; `watch_heard` and `clipping` cover the whole take; the readings are those `session` gives for the take, null when no regular beat is found or the stretch after settling is too short); `record/1` also had `verdict` (`ok`, `no_watch_heard`, `clipping`) and left a take with no watch heard or clipping out of the session; see [`record`](#record) |
| `session` | `session PATHS --json` | `input.settings.calibre` (the table calibre used, from `--calibre` or the session file's `calibre` line) and `calibre_notes` (where explicit values disagree with it), per-position `readings`, Witschi's characteristic values (`d_rate`, `d_amplitude`: the largest difference between any two positions; `dv_rate`, `dh_rate`: the same among the vertical or the horizontal positions; `dvh_rate`, `dvh_amplitude`: the vertical mean minus the horizontal mean), `isochronism` per position across states of wind (`from_wind_h`, `to_wind_h`, `rate_change` s/d, `amplitude_change` deg, `beat_error_change` ms) and `findings` (`code`, `recording`, `title`, `evidence`: conditions found in the sound, such as `no_regular_beat`, `clipping` or `cycle_wheel`, with no severity or advice); each measurement's `amplitude_states` and `rate_states` give the two-state finder's figures: `two_levels` (all the tests hold), `blocks`, `block_s`, `low`, `high`, `low_share`, `separation` (Ashman's D), `delta_bic` (BIC of one level minus two), `switches`, `dwell_low_s`, `dwell_high_s`, `switch_every_s`, `spread`, `period_s`, `regularity`, and for amplitude `tick_change`, `tock_change`, `beat_error_unlock_ms`, `beat_error_drop_ms`, `tick_unlock_ms`, `tock_unlock_ms` (each a low and high pair), `tick_split_alone`, `tock_split_alone`, `tick_edge_split_alone`, `tock_edge_split_alone`, `one_sided` (one side carries the split alone) and `unlock_jump` (the unlock beat error jumps between the levels while the drop's does not); see [sessions.md](sessions.md#two-states). Schema `timegrapher.session/2`: no tolerance, limits, verdicts, quality factor N, finding severity or advice, or two-state verdict or confidence, and no `--tolerance` flag; a session file's `tolerance` and `[limits]` are read and ignored. Also written to `summary.json`; see [sessions.md](sessions.md) Each reading's measurement has `breaks` too, with the same meaning, when its stretch holds one, and `masked_s` (the seconds masked out of the measured stretch by the session file's `masked`; present only when some are) with the caveat `masked`. Each reading's `measurement` also carries `confidence` (`rate`, `amplitude`, `beat_error`, each `se`, the standard error in the reading's unit, as `analyze` gives it; `null` when no regular beat is found) and `caveats[]` (as for `analyze`; `lift_unknown` when no `--lift`, session-file `lift` or known calibre gave the lift angle). A take kept as its analysis alone (`audio = false` in its session file, an `analysis/` folder and no sound; see [sessions.md](sessions.md#a-take-kept-as-its-analysis-alone)) is read from its kept beats, and `input.settings.analysis_only` gives `note` (no sound was kept) and `measured_by` (the software that measured it, whose beat shape each reading's `shape` is); it is `null` for a take with its sound. |
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
without the maker, one of its `family` names, or one of its
`former_names` with or without the maker, ignoring case, spaces and
punctuation. An unknown name exits 1. `calibres --search QUERY` lists
the calibres that answer a looser query, best first: every word must
match a name, another name, or the maker under any of its names
(`data/makers.json`: "DD" for Dandong, "AP" for Audemars Piguet), so
"dd 3235", "DD3235" and "AP 3120" all work.

Wheel periods and calibres the user adds in the app are kept in
`calibres.json` in the settings folder (the one `TIMEGRAPHER_CONFIG_DIR`
overrides) and read over the table: `calibres NAME` lists every wheel
with its period and whether it comes from the table, the user, or is
still unknown. They stay on the computer and nothing is sent on its own.
The core builds a pre-filled GitHub issue address from them
(`calibre_store::Catalog::contribution_url`) for the person to open and
send themselves.

`analyze --calibre NAME` sets the beat rate, the lift angle and the
escape wheel teeth from the selected catalogue entry (52° and 15 teeth where it gives
none) and names `timing_periods` and `amplitude_periods` after that
calibre's wheels. `--bph`, `--lift` and
`--escape-teeth` still win over it. Without a calibre, `analyze` guesses
the beat rate and uses 52° and a 15-tooth escape wheel. The calibre and
the settings used are in `input.settings` (`calibre`, `bph`, `lift_deg`,
`escape_teeth`). A watch whose rate differs from its calibre's (a
modified movement) needs `--bph`.

Direct `analyze`, `long` and `series` read the same custom names, aliases and
wheel overrides as `calibres` from `calibres.json`. A custom entry takes
precedence over a matching shipped name, following the existing catalogue.
Each command uses one resolved snapshot. `long` and `series` use the selected
train for naming periodic components; their `--calibre` does not set BPH or
lift defaults. Explicit `analyze` flags still win, including an explicit 52°
lift and escape-teeth override. Without `--calibre`, an unreadable user
catalogue does not affect the command's defaults; with it, catalogue errors
are reported before reading audio.

`record` and `session` also accept the same custom names/aliases and wheel
overrides. A session resolves its catalogue once for all positions; flags
win over persisted manifest values/context, then catalogue values fill missing
settings. An explicit unknown calibre is an error; an unknown manifest
calibre remains a descriptive label and supplies no assumed catalogue data.
Regression benchmarks deliberately use only the shipped catalogue, so the
machine's custom entries or malformed user store cannot alter the benchmark.

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

Store loading and saving require schema `timegrapher.clocks/1` and validate
all entries: `ppm` must be finite with `1 + ppm * 1e-6 > 0`, `ppm_sd` must
be finite and nonnegative when present, and `span_s` must be finite and
nonnegative. Missing/null `ppm_sd` means unavailable; zero ppm, zero standard
error and a manual entry's zero span remain valid. These are mathematical
domains, not calibration quality limits. An invalid store produces a path
and field error rather than an empty store, and cannot be overwritten by
`set`, `forget` or a saved measurement. Invalid values supplied programmatically
are rejected before creating directories or writing a store. This is boundary
validation, not automatic validation of every constructed `DeviceClock`.

`measure --save` and `from-log --save` validate the store before device/audio
work, then reload it before merging the result so intervening changes are
retained. A store that becomes invalid meanwhile prevents saving. Without
`--save` these commands do not need the store. An explicit `--card-ppm`, including
zero, still bypasses stored correction lookup. A fresh desktop microphone
session loads the store before querying/opening devices or replacing its
current context; load errors stop the start and are shown. A resumed session
retains its already-selected correction even if the store has changed. Keeping
a measured correction changes the app's selected correction only after a
successful save. Forgetting it in the app's Settings (Sound Card Clock)
removes the entry from the store and, when it is the input of the
microphone session in hand (listening or stopped), clears the app's
selected correction too. No correction is selected when a valid store has
no matching input. Fix an invalid store explicitly; these operations do
not repair it.

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
```sh
timegrapher record --watch NAME [--library DIR] --position DU [...]
```

Records one position of a test, for `--seconds` (default 120), and stops.
It listens only while it runs, says on stderr what it is recording and
for how long, and prints `done: the watch can be moved` when the watch may
be turned. An agent runs it once per position, after telling the person
which way to turn the watch, and never on its own initiative.

Each run makes one take folder in FOLDER holding the same files as a
recording the desktop app saves (`audio-001.wav`, `clock.csv`,
`session.json`, and a `session.toml` the app opens it with; a take paused
and resumed in the app also has a `breaks` list in `session.json`,
described in [app.md](app.md)). `record` names the folder after the start
time, the watch (when given) and the position, as in
`2026-10-08T21-30-00_Yacht-Master_DU`. The app names its saved copy after
the start time, then the watch's name and the calibre when they are set,
with no position (`2026-10-08T21-30-00_Yacht-Master_Rolex_3235`). An app
copy saved after a finished Positions Test run just after winding also
holds `positions-test.json` (format `timegrapher.positions-test/1`), that
test's results for the same test 24 hours later; `record` never writes
it. Its keys, among them the watch, calibre, beat rate, lift angle and
the test's settling and measuring times, are in [app.md](app.md). The take
is analysed at once. Whether a regular beat is found
(`watch_heard`, with the timing scatter behind it) and clipping are
measured over the whole take, so a short take still gets them.
The readings come from the stretch `session` will read: the check skips
the take's `settle_s` from the session file (30 s in a file `record`
writes; at most half the take) and reads the rest on the sound card's
clock, as `session` does, so the check and the report agree on a take
`record` creates (they can differ where `session` reads it otherwise, as
[sessions.md](sessions.md) lists); they are
null when no regular beat is found or that stretch is too short to hear
the watch on its own. Every take is added to `FOLDER/session.toml`
(written on the first take with the watch, calibre, bph, lift and
`settle_s = 30.0`, then appended to, comments and all) with its position,
`wind_h` when given, and start time, and a comment line above the entry
for each fact worth seeing there ("No regular beat found (timing scatter
X ms, Y% of a beat)", "N% of beats clipped"); the exit code is 0. Whether
to record a position again is the agent's call from those facts.
`timegrapher session FOLDER` then reads every take in the session.

`--calibre` fills bph and lift from the calibre table when they are not
given. A session file already in FOLDER sets them for every take; a
`--bph` or `--lift` that disagrees with it is an error, so one session is
measured one way. `--wind-h` is the hours since full wind (0 for the
fully wound half of a test, 24 for the run-down half); without it the
take carries no `wind_h`, and the report says the state of wind was not
given. `--file` reads a
recording instead of the microphone, at once rather than in real time,
to try the command.

Without a FOLDER, `--watch NAME` names a watch in the app's library
(found as `watches show` finds it: by its name, ignoring case and spaces
at either end; the library is `--library DIR`, default `Timegrapher Data`
in the home folder). The session folder is made among that watch's
takes, named by the date and time it started in this computer's time
(`2026-10-22 09.15`, with ` 2` and so on added when that name is taken;
unlike the app's one-position takes, no position is added, since more
positions can go into the folder later), and the watch's calibre, beat
rate and lift angle fill in those not given; the session file's `watch`
is the watch's name. The `--json`
document's `library_watch` names the watch (null when FOLDER was given),
and `session_file` is in the new folder: give that folder as FOLDER to
record the next position into the same test. The session folder is then
one take in the library, with the positions of every take in it; it has
no `session.json` of its own, so `watches` lists it with no start time,
length or `at_end`. Recording into the library holds its lock for the
whole take, so it is refused (exit 1, before anything is made) while the
app has the library open, as a second copy of the app would be; the
same holds for a FOLDER that is one of a library watch's takes.

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
| `reading` | The live reading as the app shows it: `rate_s_per_day`, `amplitude_deg` (`amplitude_error_deg`), `beat_error_unlock_ms` (the headline) and `beat_error_ms` (from the drop), `bph`, `beats_used`, `span_s`, `jitter_us`, `timing_scatter_fraction` (as for `analyze`), `snr`, `clipped_beats`, `clipped_fraction`, `watch_heard`, `rate_error_s_per_day` (the scatter of 10 s windows over the run so far scaled to `average_s`, `null` until about 30 s of unbroken beats), `beat_error_unlock_error_ms` (the standard error of the 2 s windows) and `caveats[]` (as `analyze` gives them, over the averaging time); readings are null until enough beats are heard, and whenever `watch_heard` is false (no regular beat found over an averaging time of 10 s or more, as `analyze` nulls them; under 10 s regularity is not assessed) |
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
| `bph`, `beats_expected`, `beats_found` | the beat rate used and the beats found |
| `tick_level_dbfs`, `noise_level_dbfs`, `tick_to_noise_db` | ticks against the background between them (high-passed at 1.5 kHz) |
| `signal_x` | median beat peak over the median envelope, the desktop app's "signal N×"; pure noise reads about 2× (see [validation.md](validation.md#signal-above-noise-signal_x-what-the-added-noise-tests-showed)) |
| `gap_rise_db` | background late in the gap over early in it, dB |
| `rate_s_per_day`, `beat_error_ms` | from a few seconds, on the sound card's clock |
| `suggested_gain_change_db` | the change `--apply` makes: it sets the peak to about −10 dBFS (a −6 dB step when clipped) |
| `issues[]` | the conditions detected, each `code`, `title` (the condition with its figure) and `evidence` (the measurements) |

Issue codes, each a condition detected: `silent` (peak under −80 dBFS),
`too_quiet` (peak under −30 dBFS), `clipping` (three or more clipped
samples), `hot` (peak above −6 dBFS, not clipped), `agc_suspected` (the
background rises more than 3 dB between ticks), `no_ticks` (no regular
beat found: fewer than half the beats expected, or `signal_x` under 3×).

On Linux, `mixer` holds the sound card's ALSA controls (read with
`amixer -c CARD scontents`; the card comes from `--card`, the device id, or
the only USB sound card), and each entry in `fixes` has the exact `amixer`
command. Automatic gain is turned off first and alone, because levels
measured while it acts mean little. On macOS and Windows the fixes say
which setting to change. Every fix has `requires_consent: true`.

With `--apply` the commands run and, when listening live, the check runs
again and appears as `after`. `doctor` exits 0 whatever it detects.

Example (illustrative): the C-Media USB timegrapher microphone arrives with automatic gain
on and the level at 16/16, and clips every tick:

```
$ timegrapher doctor
Level        peak 0.0 dBFS, RMS -10.7 dBFS, 10731 clipped samples
...
Detected     Clipping: 10731 samples (10731 samples clipped, peak 0.0 dBFS)
Mixer        card 2: 'Mic' 16/16 (+23.8 dB), 'Auto Gain Control' on
Changes --apply makes:
  - Turns off 'Auto Gain Control' on card 2, so the gain stays fixed.
      amixer -c 2 sset 'Auto Gain Control' off
Nothing was changed; --apply makes these changes.
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

## `watches`

```sh
timegrapher watches list [--all] [--library DIR] [--json]
timegrapher watches show NAME [--library DIR] [--json]
```

Lists the library the app keeps its watches and their takes in: schema
`timegrapher.watches/1` for `list`, `timegrapher.watch/1` for `show`. The
library is `Timegrapher Data` in the home folder, or the folder
`--library` names (a library chosen elsewhere in the app's Settings has
to be named this way); `library` is its absolute folder, and every
folder in the document is absolute too. It only reads: it takes no lock
and writes nothing, not even the library's cache, so it can run while the
app has the library open. `list` leaves archived watches out unless
`--all` is given; `show` finds a watch by its name, ignoring case and
spaces at either end, archived or not, and `show Unsorted` gives the
takes Quick Listen made (`unsorted` true, `name` `"Unsorted"`,
`archived` false, `fields` empty, `photos` 0, and `calibre`, `bph`,
`lift_deg` and `notes` null).
A field the user never set is `null`. Each take's `path` is a standard
take folder to pass to `session`, `long` or `series` as it is; `audio` is
false for a take kept as its analysis alone, which can't be measured
again from its sound; `at_end` is the readings the app showed when the
take was saved (`rate_s_per_day`, `amplitude_deg`, `beat_error_ms`
signed, from the unlock), not a new analysis, and `null` for a take saved
without them. A take is `incomplete` when a file its session file lists
is not there, or a folder it lists holds no sound. A NAME that matches no
watch, or a folder with no library in it, exits 1 with one line on
stderr naming it. The text form of `list` gives each watch's calibre,
number of takes, the UTC start of its newest take and its size; `show`
gives the watch's details and each take on a line.

## Building without sound input

Sound input uses `cpal` behind the default `live` feature (on Linux it needs
the ALSA development package, `libasound2-dev`). `cargo build
--no-default-features` builds without it, and without `update`; `doctor
--file` still works.

## Later

- `schema KIND`: print a JSON Schema for each document.
- An MCP server, if live streaming to an agent or use from chat-only apps
  turns out to need it; it would wrap the same core.

## Explicit scalar input domains

Before command execution, explicit analysis/acquisition arguments are checked for
mathematical and representation constraints, without adding empirical watch ranges:

- `--bph`: a positive `u32` (1 through 4294967295), for `analyze`, `shape`,
  `profile`, `long`, `series`, `doctor`, `record`, `session`, `synth` and `report`.
- `--lift`: finite and positive, for `analyze`, `shape`, `profile`, `long`,
  `series`, `record`, `session` and `report`. There is no added upper operating limit.
- `--escape-teeth`: a positive `u32`, for `analyze`, `long` and `series`.
- `--card-ppm` (`analyze`, `long`) and the positional ppm in `clock set`:
  finite, with `1 + ppm × 10⁻⁶ > 0`, so the clock scale advances time and
  the correction denominator is positive. Zero and admissible negative ppm
  remain valid; this does not establish calibration accuracy.

CLI validation happens during argument parsing, before audio/device access,
output creation or storing a correction. MCP validates exposed measurement BPH
and lift arguments before invoking the CLI; its schema declares the same domains.
Defaults, explicit 52° lift and zero correction keep their existing meanings.
Passing these domain checks does not guarantee every subsequent numerical
operation is representable for extreme inputs or that a watch uses those settings.

This is an explicit-argument contract. Separately, consumed session manifests
validate `bph`, `lift`/`lift_angle`, `escape_teeth` and `card_ppm` against the same
four domains. CLI session/record reject invalid stored values before audio or
append/report writes, even if flags would override them. Desktop opening validates
an applicable manifest before replay/analysis, retains unrelated valid-TOML/no-manifest
behavior, and rejects corrupt manifest syntax rather than silently defaulting.
See [sessions.md](sessions.md) for applicability and compatibility details.
Custom calibre catalogues, clock stores, other manifest numbers and internal
programmatic configurations remain outside these checks. Filter parameters and other
numeric arguments are not covered by these checks. Acquisition duration checks
remain as documented separately.

`report --bph/--lift` also use these domains: the report forwards these settings
to its analysis subprocesses, rather than storing them only as historical metadata.
The checks apply consistently even with `--no-audio` or without a recording.
MCP `make_report_bundle` applies the same BPH/lift rules before child spawn.
