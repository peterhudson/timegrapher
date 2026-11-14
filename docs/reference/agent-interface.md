# Driving the timegrapher from an AI agent

Most people who use this tool will have an AI agent to help them: to get
the microphone working, run tests and explain what the numbers mean. The
command-line tool is that agent's interface. This page is the contract.
The [watchmaker skill](../../skills/watchmaker/SKILL.md) builds on it.

## Rules

- Every command has `--help`, and `timegrapher --help` lists them all.
- Every command that reports something takes `--json` and then prints
  exactly one JSON document on stdout. Progress and warnings go to stderr.
- Fields are only added. Every field path of `analyze`, `shape`,
  `profile`, `long`, `series`, `session`, `calibres`, `watches` and `excerpt` is
  kept in `crates/timegrapher-cli/tests/json_shapes/`, and a field
  renamed or removed fails the tests.
- Exit codes: `0` success, `1` error (message on stderr), `3` from
  `regress` when the engine moved away from tg ([regress](../developer/regress.md)).
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
detected; the [watchmaker skill](../../skills/watchmaker/SKILL.md) interprets
it), and `record/2` keeps every take. Other kinds are at version 1. `input.files[].recording` appears when the recording has a
provenance sidecar (`take.wav.json`, written by `doctor --save`), carrying
the device, its configuration and the mixer settings at the time.

The additive `bph_set` field records the rate supplied through an option, a calibre
or a session file; `null` means automatic detection. It appears on `analyze`,
on each `session` reading (also inside an analyzed take), on `record.check`, and
on a live reading. `bph` and `timing_scatter_fraction` retain their numerical
meaning. When no regular beat was found and `bph_set` is null or absent, text
gives the timing scatter in ms without a percentage of a guessed beat (#857).

`software` tells builds apart: `git_hash` is the short commit hash
(`"unknown"` in a build from a source tarball, without git), `git_dirty`
whether the work tree had uncommitted changes (`null` when unknown), and
`build_date` the UTC date of the build; `timegrapher --version` prints the
same, e.g. `timegrapher 0.1.0 (8ec7b89, built 2026-10-10)`. These are not
readings: anything that compares two documents should leave `software` out.

| Kind | Command | The command's own fields |
|---|---|---|
| `analyze` | `analyze FILE --json` | Schema `timegrapher.analyze/2`. `duration_s`, `sample_rate`, `bph`, `beats_found`, `beats_found_fraction` (`beats_found` over the beats the recording's length holds at `bph`), `watch_heard` (false when a regular beat was looked for and not found: with 10 s of sound or more, the timing scatter `jitter_us` is 2% of the beat period or more, as on an empty stand; with less than 10 s, the scatter is not assessed and `watch_heard` is false only when neither the Tick's nor the Tock's averaged sound shows an unlock or a drop, looked at over the first 60 s of the recording; otherwise it is true), `overall` (`rate_s_per_day`, `beat_error_ms`, `period_s`, `jitter_us`), `rate_p05`, `rate_p95`, `jitter_us` (the timing scatter, median over the 10 s windows, µs), `timing_scatter_fraction` (`jitter_us` as a share of the beat period, 0 to 1; `watch_heard` is false when it is 0.02 or more), `amplitude_deg` (+ even/odd), `amplitude_se_deg` (+ even/odd: the standard error from the spread between 2 s windows; precision, not accuracy, which rests on the lift angle), `lift_deg`, `timing_periods`, `amplitude_periods`, `clipping` (`samples`, `beats`, `beat_fraction`, `peak_dbfs`), `usb_clock` (`ppm` the sound card runs fast, `lines`, `spread_ppm`: read from the USB 1 kHz lines in the recording, `null` when it has none), `rate_s_per_day` (the whole-recording rate on the card's clock, as tg reads it; the same figure as `overall.rate_s_per_day`), `rate_corrected_s_per_day` (that rate corrected with `usb_clock`), `doubts` (`clipped`, `beats_missing`, `lift_unknown`, `clock_unknown`: conditions detected, as flags; each one set is also in `caveats`), `confidence` (`rate`, `amplitude`, `beat_error`, each `se`, the standard error in the reading's unit; `null` when no regular beat is found, and each also `null` until the recording holds 30 of its values: the rate's are every other 10 s window, so its `se` needs about 300 s of beats and a recording of a minute or two has none; the amplitude's and beat error's are 2 s windows, about 60 s), `shape` (`even` and `odd`, the Tick and the Tock, each `windows_measured`, `share_with_sound_1`, `share_with_sound_2`, `i12_ms`, `i13_ms`, `ratio13`, `tail_ratio`; and `beat_error_first_quarter_ms`, `beat_error_last_quarter_ms`, `beat_error_quarter_windows`: the unlock beat error early and late in the recording; see [fault-signatures.md](fault-signatures.md); `null` when no regular beat is found). A `clock` block (`ppm`, `from`, `rate_s_per_day`) appears when the sound card's clock is read from the USB lines, or else a correction is stored or given, as `session` picks it for a take with no clock log. When `watch_heard` is false every reading is `null` (`overall`, the amplitudes, `beat_error_unlock_ms`, `rate_p05`/`rate_p95`, `rate_s_per_day`, `rate_corrected_s_per_day`, `confidence`, `shape`) and `timing_periods` and `amplitude_periods` are empty; `jitter_us`, `timing_scatter_fraction`, `bph` and `beats_found` stay (the rate listened for and the sounds found, not a watch's readings). `bph_heard` appears when a set beat rate (`--bph` or `--calibre`) differs from the rate the recording sounds like, guessed where the watch starts, after any lead of noise left out (`lead_left_out`). `beat_error_unlock_ms` is the signed beat error measured from the unlock (the median over the 2 s amplitude windows), the figure checked against tg's beat error in regress; `overall.beat_error_ms` is the drop's. Sign, for both: positive means Tick comes late, so the half-beat from Tock to Tick is longer than the one from Tick to Tock, by twice the value. Each run starts positive: Tick is the beat that comes late over the run's first stretch whose beat error at the drop is 0.1 ms or more (32 steady beats of each side, about 8 s at 28,800 bph), the same in `analyze` and in a live reading of the same take; nearer no beat error than that, Tick is the run's first beat. So the sign is negative only where the beat error turned over later in the run. The sides are named by the drop's timing, so `beat_error_unlock_ms` can read the other sign where the unlock and the drop differ (6 of the corpus's 183 readings). Show the size. Over a stretch with holes, each side of a hole is measured on its own and the beat errors combine as their beat-weighted mean (`overall.beat_error_ms`) or the median over all their 2 s windows (`beat_error_unlock_ms`); where the sign turns over across a hole (as the drop's fit finds it: only a stretch whose beats span 10 s or more is fitted, and the unlock follows the same decision), they combine as sizes and are given as positive, so the turn does not cancel the size the watch has either side, and the caveat `beat_error_as_size` says so. A `session` measurement's two beat errors combine the same way, and so do `long`'s; where they do, `series`' beat error readings are each given as a size, and both carry the same caveat. Where the beats jump, at a break, the sides keep their names across it by their sound (where they are not told apart by their sound, by the beat error's sign, kept the same either side of the break when it is 0.1 ms or more on both sides and its size changes by under half; otherwise by the beat count across it, and the caveat `sides_by_count` says so), the break counts in `breaks`, and the rate is fitted on each side of it apart. A break is either a pause (listening stopped and started again, as a take's `session.json` or clock log records it) or a jump with no pause recorded within 5 s of it: Tick and Tock both stepping more than 4 ms off their expected times together, as where the watch stumbles or part of the sound was lost unrecorded; such a jump is a break of unknown length, `gap_s` 0. `caveats[]`: conditions detected, each with a `code`: `no_watch` (no regular beat found; readings withheld; `reason`: `scatter`, the timing scatter is 2% of a beat or more over 10 s of sound or more, or `no_beat_sound`, under 10 s of sound neither side's averaged sound shows an unlock or a drop), `set_rate_disagrees` (`heard`), `short_span` (`span_s`, the beats span under 10 s), `clipped` (`fraction`, more than 1% of beats clipped), `beats_missing` (`fraction` found, under 95%), `lift_unknown` (the lift angle is the default), `clock_unknown` (the sound card's clock is not corrected), `clock_sources_disagree` (`usb_ppm`, `log_ppm`, `log_ppm_sd`, and `against`: on a `session` reading and in `long`'s and `series`' `caveats[]` when the recording's USB lines correct the rates and its clock log, which would correct them on its own with 10 or more entries and a standard error of 20 ppm or less, measures the card more than 5 ppm and more than twice its standard error differently, with `against` left out; and, with `against` `correction`, on those and on `analyze` when a recording with no clock log has USB lines and a stored or given correction more than 5 ppm apart, `log_ppm` being the correction and `log_ppm_sd` 0; the rates stay corrected by the USB lines), `breaks` (`count`: breaks in the stretch, pauses and jumps alike; breaks with under 5 s of the watch heard between them count once, as after one silence the beats can step more than once before they are found again), `breaks_unlisted` (`why`: `unreadable`, the take's `session.json` cannot be read or is not JSON; `entries_skipped`, some of the breaks it lists give no place or gap; `not_the_take`, the files read are not the whole take it describes: so the breaks, or those entries, come from the clock log, which shows a pause only where it lost 0.5 s or more; never on `analyze`, which reads no take, but on a `session` reading and in `long`'s and `series`' `caveats[]`), `pauses_unseen` (`count`: pauses the take's `session.json` lists, in the stretch, that its clock log shows no gap at; the log is the better witness, so each is still a break but adds no time; with no clock log, or one with no entry before or none after the pause (a log that ended early or started late), the listed gap is added and this is not given; on a `session` reading and in `long`'s and `series`' `caveats[]`), `sides_by_count` (`count`: of those, the ones where Tick and Tock were not told apart by their sound or by the beat error's sign, so the sides after each are named by the beat count across it), `masked` (`seconds`: stretches masked out of the readings by hand, as a session file's `masked` or the app's Paper Strip leaves out), `beat_error_as_size` (the beat error's sign differs either side of a cut, so `overall.beat_error_ms` and `beat_error_unlock_ms` are sizes, positive, not signed), `lead_left_out` (`seconds`: the recording opened with a lead of noise before the watch, and the readings start where the watch is heard, that many seconds in; given where the first 10 s windows' timing scatter is 0.8% of a beat or more until one under it, from which the windows' median stays under it for at least 20 s, and that one scatters at most 4 times as much as the windows from it on (taken as at least 0.02% of a beat), and so does the next window clear of it, with no jump of the beats from the first's start to the second's end and every 5 s window between them there, so a noise window that dips under 0.8% stays in the lead; a watch whose first windows scatter more than 4 times its later ones has that rough start left out the same way; also on a `session` reading, from its settled start, then measured from the entry's start). Apart from `lead_left_out`, none of them changes a reading or its ±. Empty when none is detected. Version 2 removed `confidence.*.grade`, `faults[]` and every grading rule. `analyze` and `session` share one beat pipeline (the chunked stream that `long`, `series` and `regress` read too), so the same file gives the same beats and readings in both. The top level reads the whole file, with no settling, less a lead of noise before the watch (`lead_left_out`); when the session file in the file's folder lists it, `take` says what `session` reads of it: `session_file`, `readings[]` (one for each entry naming the file, an entry naming the take's own folder, `.`, naming it when it is the folder's only sound file; each as `session`'s `readings[]` gives it, read with `--bph`, `--lift` and `--calibre` as `session`'s own and no cycles, so its figures are `session`'s to the bit) and `left_out[]` (as `session` gives it); when the session file cannot be read, or `session` would refuse an entry naming the file, `take` is `session_file` and `error` (the reason as text) instead, and the file's own reading stands. No `take` when no session file lists the file. |
| `calibres` | `calibres [NAME \| --search QUERY] --json` | `calibres[]` (the shipped table): `calibre`, `maker`, `family` (other names that match), `former_names` (names the row had in earlier versions of the table, which still match; left out when there are none), `common`, `bph`, `lift_angle_deg`, `escape_teeth`, `seconds`, `wheels[]` (`name`, `period_s` for one full turn, `teeth`, `pinion`, `basis`), `sources`, `confidence`, `notes`; `custom[]` (calibres the user added: `name`, `maker`, `aliases`, `bph`, `lift_angle_deg`, `escape_teeth`, `wheels[]`, `notes`); with NAME, `slots[]` (`name`, `period_s`, `origin`: `shipped`, `user`, `counts` (worked out from tooth and leaf counts where nobody gave the period; `basis` then says how) or `unknown`, `shipped_period_s` when the user's replaces the table's, `basis`) and `train` (as `train`'s document gives it, below); `listed` (`common` with neither NAME nor `--search`, which lists the common calibres and the user's; else `matching`), `table_size` (calibres in the shipped table); `input.settings.user_calibres` is the file read; a figure no source gives is `null` or left out (below) |
| `train` | `train [--calibre NAME] [--bph N] [--escape T/L] [--fourth T/L] [--third T/L] [--centre T/L] [--barrel T] [--period ARBOR=S,...] --json` | Schema `timegrapher.train/1`. A going train worked out from its tooth and leaf counts (core `train::solve`), starting from the calibre's counts and periods (the table's with the user's over them) where `--calibre` is given; each flag replaces that arbor's. `bph` (given, or the calibre's), `bph_from_counts` (with no beat rate: the escape wheel's teeth × 7200 over its turn, where the counts carry a period to it), `arbors[]`, escape wheel outward (`name`, `teeth`, `leaves` of the pinion on the same arbor, `typed_s` the period given or the table's, `counted_s` the period the counts give, carried from the nearest start in meshes (the beat rate at the escape wheel, or another arbor's given period; the one further in on a tie; never the arbor's own), so a wrong count reads otherwise only on the wheels either side of it; `null` on an arbor with no period given whose nearest starts inward and outward give periods that read otherwise, to the thousandth, or whose one start is a given period its own `counted_s` reads otherwise, which then has `clash` (`inward` and `outward`, `null` for a side with no start, each `via`, the neighbouring arbor or `beat rate`, `from` and `period_s`), `from` (`kind` `beat_rate` or `period`, with `arbor` the given period it starts from), `step` (the last step: `kind` `escape` with `bph` and `teeth`, or `mesh` with `arbor`, its `period_s`, the mesh's `teeth` and `leaves`, and `outward`), `needs` (with no period, given or counted: `arbor` and `count`, `teeth`, `leaves` or `period`, the count that would carry a period there)), `meshes[]` (`wheel`, `pinion`, `teeth`, `leaves`, `period_s`: the wheel's turn over its teeth, one tooth every so often). The escape wheel turns in `teeth × 7200 / bph` s; each arbor out turns in the next one in's period × its wheel's teeth ÷ that arbor's pinion leaves. Nothing is saved. |
| `shape` | `shape FILE --json` | per pallet stone (even/odd beats): unlock, impulse and drop timing and level; see [fault-signatures.md](fault-signatures.md) |
| `profile` | `profile FILE --at S --span S --json` | `a` (the tick: even beats) and `b` (the tock: odd beats): `beats`, `t0_ms`, `step_ms`, `median`, `p10`, `p90` (envelope point by point), `floor`, `unlock_ms`, `drop_ms`, `peak_ms`, `amplitude_deg`, `sound1_ms`, `sound2_ms`, `sound3_ms`; times in ms from the beat. `--svg FILE` also draws them |
| `long` | `long FILES --json` | `duration_s`, `sample_rate`, `bph`, `lift_deg`, `beats_found`, `clean_fraction` (the share of the expected beats found with a clean match, 0 to 1), `clock`, `overall` (`rate_s_per_day`, `beat_error_ms`, `period_s`, `jitter_us`, `beats_used`), `beat_error_unlock_ms`, `rate_p05`, `rate_p95`, `amplitude_deg`, `amplitude_p05`, `amplitude_p95`, `rate_periods[]` and `amplitude_periods[]` (`period_s`, `resolution_s`, `significance`, `beyond_simulated` (true when the search was held to the simulated null and its false alarm is under 0.0003, the smallest it was checked to: `significance` is then the fitted tail's extrapolation, and the text says the line is beyond the simulated range), `harmonics`, `explained`, `peak_to_peak` (s/d for the rate, degrees for the amplitude), `timing_peak_to_peak_ms` (the rate's only), `size` and `size_unit` (the size to show: `peak_to_peak`, except a rate cycle shorter than 30 s, stated as its timing swing in ms), `wheel`, `nearest_wheel`), `caveats[]`; together, rate and amplitude over the run, clock calibration, periodic components named after wheels (`--calibre`; with no calibre none, or only `--escape-teeth`'s escape wheel; `input.settings.named_wheels`; `input.settings.named_meshes`, the calibre's meshes to mark beside them (`name`, the pinion; `wheel`; `period_s`; `about`, its hover text), empty without its tooth and leaf counts, see [long-runs.md](long-runs.md); and `input.settings.reference_lines`, 1 min and 1 h as `label` and `period_s`, when no wheel is named) with false-alarm probability; also written to `summary.json` beside `report.html`; see [long-runs.md](long-runs.md) Without a clock log, the clock is corrected as `analyze` corrects it: a stored or given error, else the recording's USB lines. tg and a session's `reference` rates are on the sound card's clock. `breaks` (`audio_s`, `gap_s`; present only for a take that was paused or resumed or whose beats jump, as for `analyze`; a jump with no pause recorded near it has `gap_s` 0): times run on with each gap in, rate and beat error are fitted on each side of a break apart (the beat error combined as for `analyze`, and where it combines as sizes the slices' beat errors are sizes too), and no window or slice spans one. `clock.breaks` lists the pauses the clock log shows. `caveats[]`: `sides_by_count` (`count`), `breaks_unlisted` (`why`), `pauses_unseen` (`count`) and `beat_error_as_size` (the take's beat errors, its slices' and `series`' beat error readings are sizes, positive, not signed), and `positions` (`count`) as for `series` as for `analyze`; empty when none. |
| `series` | `series FILES --json` | schema `timegrapher.series/3` (version 2 removed `verdict`, the findings' `severity` and `advice`, and `moves_with`; version 3 removed the isochronism's quadratic: `curve_centre_deg`, `curve`, `curve_explained` and `turn_deg`); `clock` (`ppm`, `rate_error_s_per_day`, `from`, `tracks_drift`) describes the fit actually applied, or is `null` when uncorrected. Positive ppm means the card runs fast; for a drifting log, ppm summarizes the fit. No calibration uncertainty is claimed. An explicit `--clock` log takes precedence (its rate set by the recordings' USB lines when they have them), otherwise the recordings' USB lines, then the first recording's stored device correction (including zero); `series` has no `--card-ppm` or `--device` options. Per series (`rate`, `amplitude`, `beat_error`): `headline` (what the tests detected, stated as facts with their statistics), `summary` (the headline's lead in a few words, one line, no series name: e.g. "repeats every 46 s, 4.00 s/d peak to peak", "no cycle, level change or trend"), `screen_headline` and `screen_summary` (the same two as a screen shows them: each figure with a true minus, U+2212, the words as they are), `screen_stats` (the statistics behind the headline on one line, as the app's Steadiness Info shows them), `unlock_hopping`, and the tests behind it (`autocorrelation`, `allan`, `cusum`, `changes`, `trend`, `period` (with `other_periods_s`), `two_state`, `cycle`), all three on readings of the rate's length; `isochronism` (rate by 2° amplitude bins, `s_per_day_per_deg` and `s_per_day_per_deg_se` (the slope over every reading, corrected for the amplitude readings' noise; null below a `reliability` of 0.5), `reliability`, `headline`); `wind_down` (first half hour against 24 h after full wind or the last half hour, `--wind-h`: `rate_change_s_per_day`, `amplitude_change_deg`, `beat_error_change_ms`); `rate_amplitude` (`r`, `p_value`, `effective_readings`, `s_per_day_per_deg` and `s_per_day_per_deg_se` (corrected as the isochronism's; null below a `reliability` of 0.5), `reliability`, `take_r`, `take_s_per_day_per_deg`, `headline`; always given, with no threshold); `distributions` (the app's histograms over the take and each `--stretch-min` stretch: `series`, `values` (`readings` or `2 s`), `edges`, `counts`, `below`, `above`, `median`, `p10`, `p90`, `peaks`, `normal_line`, `quantiles`, `summary`); `findings` (`code`, `series`, `title`, `evidence`: one per series where structure was detected); `spectra` (per series: `series`, `unit`, `step_s`, `period_s[]` longest first, `amplitude[]`: the size of the sine at each period, at most 600 points on a log period axis; `method`: `hann_fft` (every sample present) or `lomb_scargle` (samples missing), each size the sine and a constant fitted together to the samples present with Hann weights, nothing filled in, a period whose cycle the samples cover too thinly `null`, which on a complete grid is the Hann-windowed FFT's size; `support` gives finite/total FFT input-grid samples before padding, not acoustic coverage, and `description`, the sentence the app and the MCP view show for it; the rate's also `reading_swing` (`reading_s`, `period_s[]`, `swing_s_per_day[]`: the swing each line makes in the rate readings, as the app's FFT draws it)); with `--views`, the chart data as well (each series' `views`: `readings`, `search`, `fold`; `period_span`; the whole take's distributions' `points`, `[value, z]`); with `--maps`, `time_frequency` (an STFT and a synchrosqueezed wavelet transform map of each series: `series`, `method`, `unit`, `step_s`, `window_s`, `hop_s`, `period_s[]`, `t_s[]`, `values[][]`, null where nothing was read; `scale`, the colour scale the app draws it in: `linear` and `log`, each `lo` and `hi`, the sizes the darkest and the brightest colour stand for, the brightest also for every size above it, or null when nothing was read) and `coherence` (the wavelet coherence of the rate with the amplitude and of Tick's amplitude with Tock's: `x`, `y`, `step_s`, `period_s[]`, `t_s[]`, `coherence[][]`, `phase_deg[][]`, `noise_95[]`, `noise_pairs`) and `lomb_scargle` (each series' readings' periodogram, gaps and all: `series`, `unit`, `readings`, `span_s`, `period_s[]`, `power[]`, `window[]`, `peaks[]` with `period_s`, `power`, `false_alarm`, `window_power`, `on_window_line`, `wheel`) and `wheel_turns` (each named wheel's turn averaged over the take and per 20° of amplitude: `profile[]`, `se[]`, `turns`) and `kurtograms` (each series' spectral kurtosis by period and window length: `series`, `step_s`, `window_s[]`, `windows[]`, `noise_sd[]`, `period_s[]`, `values[][]`, null where a window holds under three turns; about 0 for noise, -1 for a steady line, above 0 for one that comes in bursts); `wheel_lines` (each named wheel's line size in the beat timing, amplitude and beat error, by 10° of amplitude: `bins[]` with `from_deg`, `windows`, `size`, `se`, and for the rate `size_s_per_day`, `se_s_per_day`; every window with `--maps`) is in the plain JSON too; see [series.md](series.md) tg reference rates remain on the sound card's clock; session `reference` values are supplied by the caller, on the clock their `clock` names (`own` or `sound_card`), or on one not recorded when it is left out. `breaks` (`audio_s`, `gap_s`; present only for a take that was paused or resumed or whose beats jump, as for `analyze`; a jump with no pause recorded near it has `gap_s` 0): times run on with each gap in, rate and beat error are fitted on each side of a break apart, and no window or slice spans one; where the beat error's sign turns over across a break (as for `analyze`), the beat error series is read as sizes, given as positive. `caveats[]`: `sides_by_count` (`count`), `breaks_unlisted` (`why`), `pauses_unseen` (`count`) and `beat_error_as_size` (the `beat_error` series' readings, and so its `median` and the tests on it, are sizes, positive, not signed), and `positions` (`count`: the take's session file names that many positions, so the isochronism and the rate against the amplitude span them, and both their headlines say so) as for `analyze`; empty when none. |
| `clock` | `clock list\|measure\|from-log\|set\|forget --json` | each input's stored clock error (below) |
| `watches` | `watches list [--all] [--library DIR] --json` | the app's library of watches, read only: `library`, `bytes`, `watches[]` (`id`, `name`, `calibre`, `bph`, `lift_deg`, `archived`, `takes` (how many), `bytes`, `newest_started_utc`), `unsorted` (how many takes Quick Listen made are not yet filed under a watch), `unreadable[]`, `caveats[]` (none yet) (see [`watches`](#watches)) |
| `watch` | `watches show NAME [--library DIR] --json` | one watch, or `Unsorted`: `library`, `id`, `name`, `unsorted`, `folder`, `calibre`, `bph`, `lift_deg`, `notes`, `archived`, `fields[]` (`label`, `kind`, `value`), `photos` (how many), `bytes`, `takes[]` newest first (`path`, `name`, `started_utc`, `named_start` (only for a take with no `started_utc` whose folder's name gives its start: `2026-10-22T09:10`, in the recording computer's time, no zone; left out otherwise), `duration_s`, `positions[]`, `audio`, `incomplete`, `bytes`, `reading`, `at_end`), `unreadable[]`, `caveats[]` (none yet) |
| `watches-change` | `watches new\|edit\|add-take\|move-take\|keep-analysis-only\|delete-take\|add-photos\|delete-photo … --json` | what a change did: `action`, `watch` (`id`, `name`, `folder`), `take`, `photos[]`, `left`, `caveats[]` (see [Changing the watches](#changing-the-watches)) |
| `devices` | `devices --json` | `devices[]`: `id`, `name`, `manufacturer`, `is_default`, `default_config`, `supported[]` (`min_channels`, `max_channels`, `min_sample_rate`, `max_sample_rate`, `sample_format`) |
| `usb_agc` | `devices --agc --json` (`--agc-off`, `--agc-on` to set it) | macOS: each USB microphone's automatic gain switch, read over USB. `microphones[]`: `name`, `vendor_id`, `product_id`, `control` (`interface`, `unit`, `channel`, `volume`), `before` (the switch as read), `after` (read back after `--agc-off` or `--agc-on`, else null), `volume_db` (capture volume: current, lowest, highest), `error` (only when a request failed). On Windows the switch is the system's own control for each input that has one (the Sound control panel's "AGC"), and each entry carries only `name`, `before`, `after` and `error`. The microphone turns the switch on again when it is plugged in. On Linux the command fails and names the mixer instead. |
| `doctor` | `doctor --json` | Schema `timegrapher.doctor/2`. `source`, `check`, `mixer`, `fixes[]`, `applied`, `after` (below). Version 2 removed `verdict`, `check.signal_band`, `issues[].severity`, `issues[].advice` and the `noisy` issue. |
| `record` | `record FOLDER --position P --json` | (`record/2`) `take` (the take's folder), `library_watch` (the library watch the session folder was made under, without a FOLDER; else null), `session_file` (the session file the take was added to), `position`, `wind_h` (null when not given), `seconds`, `stopped_early` (true when `--stop-file` ended the recording), `source`, `sample_rate`, `check` (`from_s`, `to_s`: the stretch read, after the settling time; `watch_heard`, `timing_scatter_ms` and `timing_scatter_fraction` (the median timing scatter over the whole take, in ms and as a share of the beat; 2% or more is no regular beat), `bph`, `beats_found`, `beats_expected`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms` (signed: from the unlock, else from the drop where the unlock is not found, as `session`'s reading gives it), `clipping`; `watch_heard` and `clipping` cover the whole take; the readings are those `session` gives for the take, null when no regular beat is found or the stretch after settling is too short); `record/1` also had `verdict` (`ok`, `no_watch_heard`, `clipping`) and left a take with no watch heard or clipping out of the session; see [`record`](#record) |
| `session` | `session PATHS --json` | `watch`, `owner`, `notes`, `calibre`, `bph` and `lift_deg` (the session file's, as used), `input.settings.calibre` (the table calibre used, from `--calibre` or the session file's `calibre` line) and `calibre_notes` (where explicit values disagree with it), per-position `readings`, `left_out[]` (each entry with nothing to read, which has no reading: `file`, `position` (Witschi's code, as a reading's), `start_s` and `why`, the reason as text, such as a given `settle_s` that leaves its stretch nothing; empty when none is left out), `report`, which holds Witschi's characteristic values in `states[]`, one for each state of wind (`wind_h`, `positions`, `x`, `xh`, `xv`: the mean rate over all, the horizontal and the vertical positions; `amplitude_mean`, `di` (6H minus CH, s/d), `beat_error_source`, `beat_error_sources`, `caveats`, and `d_rate`, `d_amplitude`: the largest difference between any two positions; `dv_rate`, `dh_rate`: the same among the vertical or the horizontal positions; `dvh_rate`, `dvh_amplitude`: the vertical mean minus the horizontal mean; `beat_error_mean`: the mean unsigned beat error over the positions, ms; `d_beat_error`: the largest difference between the positions' unsigned beat errors, ms), and beside `states[]` `im` and `im_all` (the largest isochronism over the positions leaving out 12H, as NIHS 93-10, and over all of them, signed, s/d) and `ie` (the change of the mean rate X from the fullest to the least wound state, over the positions measured in both, s/d), `isochronism` per position across states of wind (`from_wind_h`, `to_wind_h`, `rate_change` s/d, `amplitude_change` deg, `beat_error_change` ms) and `findings` (`code`, `recording`, `title`, `evidence`: conditions found in the sound, such as `no_regular_beat`, `clipping` or `cycle_wheel`, with no severity or advice), all four inside `report`; each reading's `cycles[]` (`series`, `period_s`, `size`, `size_unit`, `significance`, `beyond_simulated` (as for `long`), `explained`, `wheel`, `nearest_wheel`, `fraction_of`, and `harmonic_of`: `period_s` and `n` of a longer cycle found in the same series whose period is this one's times `n`, as `long`'s search marks it, else `null`; a cycle so marked is a harmonic of that one, and `findings` report it with that cycle, not on its own, unless it is a wheel's own line (`wheel` set) or explains more of the variation than that cycle does: then it is reported in its own right, with the mark as a note that says whether that cycle is weaker or stronger); each measurement's `amplitude_states` and `rate_states` give the two-state finder's figures: `two_levels` (all the tests hold), `blocks`, `block_s`, `low`, `high`, `low_share`, `separation` (Ashman's D), `delta_bic` (BIC of one level minus two), `switches`, `dwell_low_s`, `dwell_high_s`, `switch_every_s`, `spread`, `period_s`, `regularity`, and for amplitude `tick_change`, `tock_change`, `beat_error_unlock_ms`, `beat_error_drop_ms`, `tick_unlock_ms`, `tock_unlock_ms` (each a low and high pair), `tick_split_alone`, `tock_split_alone`, `tick_edge_split_alone`, `tock_edge_split_alone`, `one_sided` (one side carries the split alone) and `unlock_jump` (the unlock beat error jumps between the levels while the drop's does not); see [sessions.md](sessions.md#two-states). Schema `timegrapher.session/2`: no tolerance, limits, verdicts, quality factor N, finding severity or advice, or two-state verdict or confidence, and no `--tolerance` flag; a session file's `tolerance` and `[limits]` are read and ignored. Also written to `summary.json`; see [sessions.md](sessions.md) Each reading's measurement has `breaks` too, with the same meaning, when its stretch holds one, and `masked_s` (the seconds masked out of the measured stretch by the session file's `masked`; present only when some are) with the caveat `masked`. Each reading's `measurement` also carries `confidence` (`rate`, `amplitude`, `beat_error`, each `se`, the standard error in the reading's unit, as `analyze` gives it; `null` when no regular beat is found) and `caveats[]` (as for `analyze`; `lift_unknown` when no `--lift`, session-file `lift` or known calibre gave the lift angle). Each reading's `reference` is the session file's (`rate_s_per_day`, `amplitude_deg`, `beat_error_ms`, `source`, and `clock`, `own` or `sound_card`, when given). A take kept as its analysis alone (`audio = false` in its session file, an `analysis/` folder and no sound; see [sessions.md](sessions.md#a-take-kept-as-its-analysis-alone)) is read from its kept beats, and `input.settings.analysis_only` gives `note` (no sound was kept) and `measured_by` (the software that measured it, whose beat shape each reading's `shape` is); it is `null` for a take with its sound. Each reading's `shape` also gives `from_s` and `measured_s`: the stretch of the recording, in seconds from its start, that the beat shape was measured on. `poise[]` (left out when empty): the Dynamic Poise fit of each state of wind whose upright readings number five or more (the 45° stops 1H30, 4H30, 7H30 and 10H30 with 3H, 6H, 9H and 12H; core `poise::by_state`, the fit the app's Dynamic Poise pane shows), fullest first, each `wind_h` and `fit` (`positions`, `wave` (`mean`, `size`, `peak_deg`, each `value` and `se`: the measured rate wave, s/d and degrees from 12H), `wave_clear` (whether its fastest hour is named), `heavy_spot` (the inferred poise error, or `null` with `withheld` saying why: `null_band`, `no_amplitude`, `no_fit`, `no_se`, `within_noise`), `caveats`, `amplitude_wave`, `amplitude_min_deg`, `amplitude_max_deg`, `text`, `short_text`, `heavy_text`, the words the app shows). `text_reports[]` (left out when empty): the plain-text reports written beside `report.html`, each `file` (`positions-report.txt` or `poise-report.txt`) and `text`, the whole report as core `text_report` writes it and the app's Save as Text… saves it; see [sessions.md](sessions.md). |
| `report` | `report [RECORDING] --json` | the bundle's manifest: `created_utc`, `kind`, `note`, `os_version`, `recording`, `commands[]` (`args`, `cwd`, `output`, `replayable`), `files[]` (`path`, `bytes`, `sha256`), `consent`, `redacted`, `left_out`; the zip's path goes to stdout without `--json`; see [reports.md](reports.md) |
| `replay` | `report --replay BUNDLE --json` | `bundle` (when and by which build it was made), `damaged[]`, `commands[]` (`args`, `output`, `ran`, `error`, `moved[]` of `path`, `then`, `now`) |

Tick and tock: the engine numbers beats from the first one it hears, so the tick is simply the even beats and the tock the odd ones, as tg's tic and toc are; it cannot tell which pallet stone is which. Fields named `even`/`odd` or `a`/`b` mean tick/tock in that order, and people-facing text says Tick and Tock.

Rates are seconds per day, positive gaining. Rates from a recording are on
the sound card's clock unless `long` was given a clock log or a stored
correction; a sound card can be tens of ppm off (1 ppm is 0.0864 s/d).

### Reading window coverage

Session measurements carry `coverage.rate`, `coverage.amplitude` and
`coverage.beat_error`, each with `windows_used` and `windows_in_span`.
The denominator counts nominal windows within kept time, excluding windows
that reach into gaps or masks. Missing measurements inside kept sound count
against coverage. Rate uses its 10 s windows; amplitude and beat error use
their amplitude-window grid. Beat-error counts follow the selected source.

Below 60% a measurement carries a `reading_windows` caveat naming the figure
and both counts. The existing three-value minimum for a standard error is
unchanged. `unlock_coverage` remains for compatibility; the regression
harness uses core's amplitude coverage instead of its own 60% threshold.

### Beat-error provenance

Direct readings carry `beat_error_source`: `"unlock"` when the unlock was
found, `"drop"` when the reading falls back to the drop, and `null` when
neither is read. It accompanies the analyze summary, session `measurement`,
Watches `reading`, Power Reserve `hours[]`, and live `reading`. It describes the selected
beat error; the separate raw unlock and drop fields retain their meanings.
The choice and all measured figures are unchanged.

A drop-derived reading carries `{"code":"beat_error_from_drop"}` in
`caveats[]`. Library readings and Power Reserve hours now carry the core
measurement's caveats as well as its source. Older saved versions of those
two types can lack these additive fields; they read as a null source and an
empty caveat list, without guessing provenance for an old value. Withheld
readings have no source or drop-derived caveat.

Position and state aggregates select one source as described below. Live and
saved charts keep unlock and drop series separate. Older live trend documents
without source metadata keep their values in a separate unknown-source series.

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
the beat rate from the first 10 s of sound. When a watch is heard after a
lead of noise and the sound where it starts is another rate, it reads the
recording again at that rate; when no watch is heard, it guesses again as
`session` reads a file, afresh at each 47 s chunk's sound until a watch is
heard, and reads the recording at that rate. Either way it keeps the second
reading only when it hears a watch and the first does not, or both do and
it kept a larger share of the beats its span holds (by more than 2
standard errors), or, within that, it is at twice the first's rate or
its rate fit's residuals (robust SD) are smaller, so a long lead of stand
noise before a slow watch does not set the rate (#1423). It
uses 52° and a 15-tooth escape wheel for its search, and
names no periodic component after a wheel unless `--escape-teeth` gives
the escape wheel. The calibre and
the settings used are in `input.settings` (`calibre`, `bph`, `lift_deg`,
`escape_teeth`). A watch whose rate differs from its calibre's (a
modified movement) needs `--bph`.

A sound file inside a take, one its folder's `session.toml` lists, is read
with the take's settings by `analyze`, `profile`, `shape`, `long` and
`series`, as the app opens it: a flag, then the session file's beat rate and
lift angle, then its calibre's, then the default (`take::stream_settings`,
the file found by `take_path::take_of`). A session file that can't be read
leaves `analyze`, `profile` and `shape` on the file's own settings.

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
`timegrapher.clocks/2`) in the settings folder: `$TIMEGRAPHER_CONFIG_DIR`,
else `~/.config/timegrapher` on Linux, `~/Library/Application
Support/timegrapher` on macOS, `%APPDATA%\timegrapher` on Windows. Each
entry is `device`, `ppm` (positive: the card runs fast, so uncorrected
rates read slow by `ppm` × 0.0864 s/d), `ppm_sd`, `span_s`, `measured_utc` and
`source` (`measure`, `log` or `set`). `measure` listens (nothing need be on
the microphone) and fits the audio that arrives against the system clock,
which must be kept by NTP; 20 minutes gives about ±0.3 ppm. `from-log`
takes the error from a recording's clock log. Neither stores anything
without `--save`.

A store written before #657 (`timegrapher.clocks/1`, `ppm` positive when
the card was slow) is read with each `ppm`'s sign changed, and saved back
as `/2`. Store loading and saving require schema `timegrapher.clocks/2` and validate
all entries: `ppm` must be finite with `1 - ppm * 1e-6 > 0`, `ppm_sd` must
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
successful save. Forgetting it in the app's Settings (Microphone, Quartz Correction)
removes the entry from the store and, when it is the input of the
microphone session in hand (listening or stopped), clears the app's
selected correction too. No correction is selected when a valid store has
no matching input. Fix an invalid store explicitly; these operations do
not repair it.

`analyze` adds the corrected rate beside the card's: with `--card-ppm N` (ppm fast),
`--device NAME`, or a stored entry for the input the recording names, its
document gains `clock` (`ppm`, `from`, `rate_error_s_per_day`,
`rate_s_per_day`). Every other field, `overall.rate_s_per_day` included,
stays on the card's clock, as tg reads it, so comparisons with tg and the
regression baseline are unchanged. `long` applies the correction to its
whole report (see [long-runs.md](long-runs.md)).

A clock log (`--clock`, or a recording's `clock` in its session file)
under 10 entries or measured to a standard error over 20 ppm corrects
nothing; the fit's `withheld` gives `ppm`, `ppm_sd`, `points`, `span_s`
and `reason` (`few_entries` or `unsure`). When the recording has USB
lines they set the rate even with a clock log, which then only places its
breaks; `withheld.reason` is then `usb_lines`. `measure --save` and
`from-log --save` refuse a measurement under 10 entries or with a standard
error over 20 ppm, saying what was measured; without `--save` it is still
reported.

## `record`

```sh
timegrapher record FOLDER --position DU [--seconds 120] [--device NAME]
                   [--watch TEXT] [--calibre NAME] [--bph N] [--lift DEG]
                   [--wind-h H] [--file AUDIO] [--live PATH] [--stop-file PATH]
                   [--json]
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
with no position (`2026-10-08T21-30-00_Yacht-Master_Dandong_3235`). An app
copy saved after a finished Positions Test also holds
`positions-test.json` (format `timegrapher.positions-test/1`): run just
after winding, that test's results for the same test 24 hours later; run
24 hours after, its results with `wind_h` and the full-wind results it
was set against (`against`). `record` never writes it. Its keys, among them the watch, calibre, beat rate, lift angle and
the test's settling and measuring times, are in [app.md](app.md). The take
is analysed at once. Whether a regular beat is found
(`watch_heard`, with the timing scatter behind it) and clipping are
measured over the whole take, so a short take still gets them.
The readings come from the stretch `session` will read: the check skips
the take's `settle_s` from the session file (20 s but at most half the
take when the file gives none, as one `record` writes does; one given is
kept up to the take's end) and reads the rest
on the sound card's clock, as `session` does, so the check and the report agree on a take
`record` creates (they can differ where `session` reads it otherwise, as
[sessions.md](sessions.md) lists); they are
null when no regular beat is found or that stretch is too short to hear
the watch on its own. Every take is added to `FOLDER/session.toml`
(written on the first take with the watch, calibre, bph and lift, then
appended to, comments and all) with its position,
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

`--power-reserve` makes the run a Power Reserve Test, in place of
`--seconds`: it records in the one position until the watch stops (the
last beat heard before 10 minutes with none, as the app's Power Reserve
pane finds it) or 80 hours pass, then reads the take's sound as `session`
reads it, on the take's clock log, and writes the run to the session
folder's `power-reserve.json` (see [app.md](app.md), Power Reserve: the
start, position, `ending` (`stopped`, `by_hand` for a recording that ended
while the watch still ran, or `limit`), the first and last beats heard,
`run_s` in true seconds, the silences of 30 s or more inside the run, and
`hours[]`, each hour measured as `session` measures a stretch, with
`read_s`, its seconds of whole 10 s readings). Without a FOLDER the folder
is named by the date and time and "Power Reserve". The `--json` document
adds `power_reserve` with the same contents. `session FOLDER --json` gives
it too, as `power_reserve`, whenever the folder holds `power-reserve.json`
(the app's Power Reserve takes do), and leaves the key out otherwise.

`--stop-file PATH` ends the recording early once PATH exists, as the
app's Stop does: another program creates the file (any content) to stop
it. It is looked for about four times a second. The take is kept, checked
and added to the session file as any other; with `--power-reserve` the
run ends `by_hand` and `power-reserve.json` is written with its hours, as
the app's Stop Test leaves it. The `--json` document's `stopped_early` is
true. A PATH already there when `record` starts is refused (exit 1, before
anything is made), so a stop file left from an earlier run never ends a
new one at once; `record` never removes it.

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
| `reading` | The live reading as the app shows it: `rate_s_per_day`, `amplitude_deg` (`amplitude_error_deg`), `beat_error_unlock_ms` (the headline) and `beat_error_ms` (from the drop), `bph`, `beats_used`, `span_s`, `jitter_us`, `timing_scatter_fraction` (as for `analyze`), `snr`, `clipped_beats`, `clipped_fraction`, `watch_heard`, `rate_error_s_per_day` (the scatter of 10 s windows over the run so far scaled to `average_s`, `null` until the take holds 30 of its every-other 10 s windows, about 300 s of beats; a shorter run borrows their scatter, #1470), `beat_error_unlock_error_ms` (the standard error of the 2 s windows) and `beat_error_source` (unlock, drop or null) and `caveats[]` (as `analyze` gives them, over the averaging time, plus the live-only `lock_template_kept`: the beat template built again from the first 47 s, as a saved take's reading builds it, read both sides in a share of the 2 s amplitude windows lower by more than 2 standard errors than the template live locked on with, so live kept that one, and a saved take of the same sound may read differently); readings are null until enough beats are heard, and whenever `watch_heard` is false (no regular beat found over an averaging time of 10 s or more, as `analyze` nulls them; under 10 s regularity is not assessed) |
| `strip` | The paper strip: `span_s` (30), `period_s`, and `beats` as `[time_s, lead_ms, side]` for the last 30 s. `lead_ms` is how early the beat came against a clock at the nominal rate, anchored on the first 16 beats as the app anchors its strip. `side` is 0 for the Tick and 1 for the Tock |
| `trend` | The readings about once a second as columns `t_s`, `rate`, `amplitude`, `beat_error`, and aligned `beat_error_source` (`"unlock"`, `"drop"`, or null). Beat error is signed: from the unlock, else from the drop where the unlock is not found. It keeps the whole take, up to 300 points: past that, every other point of the older half is dropped |

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
| `sample_rate`, `duration_s` | the sound read |
| `peak_dbfs`, `rms_dbfs` | level relative to full scale |
| `dc_offset` | mean of the samples relative to full scale |
| `clipped_samples` | samples at full scale, plus flat tops below it (clipping before the converter) |
| `bph`, `beats_expected`, `beats_found` | the beat rate used and the beats found |
| `tick_level_dbfs`, `noise_level_dbfs`, `tick_to_noise_db` | ticks against the background between them (high-passed at 1.5 kHz) |
| `signal_x` | median beat peak over the median envelope, the desktop app's "signal N×"; pure noise reads about 2× (see [validation.md](../research/validation.md#signal-above-noise-signal_x-what-the-added-noise-tests-showed)) |
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
app has the library open. `list` gives every watch; `show` finds a
watch by its name, ignoring case and spaces at either end, and `show
Unsorted` gives the
takes Quick Listen made (`unsorted` true, `name` `"Unsorted"`,
`archived` false, `fields` empty, `photos` 0, and `calibre`, `bph`,
`lift_deg` and `notes` null).
Deprecated: watches are kept or deleted, no longer archived. `archived`
stays in both documents, true only for a watch an older version of the
app marked and that has not been saved since (saving clears it), and
`list` no longer leaves such a watch out; `--all` is still accepted and
changes nothing.
A field the user never set is `null`. Each take's `path` is a standard
take folder to pass to `session`, `long` or `series` as it is; `audio` is
false for a take kept as its analysis alone, which can't be measured
again from its sound; `reading` is the take's own reading, its last
position as `session` reads it with no flags (`position`, `rate_s_per_day`
on the take's clock where `calibrated` is true, `amplitude_deg`,
`beat_error_ms` signed, from the unlock, else from the drop where
the unlock is not found, `watch_heard`), `null` for an
incomplete take or one that can't be read (said on stderr). It is worked
out from the take's sound once, through the stream engine as `session`
reads it (about a minute for a 9 h take; the take's name is said on stderr
first), and kept in the user's settings folder (`readings.json`), so the
library is still not written; a take that changes is read again, and so
is one whose clock correction stored for its input, calibre as the user's
calibres define it, sound or clock log named outside its folder, or
program version changes. The reading is of the take's last position with
anything to read, as `session` leaves a position out when its settling
time covers it. The text
output shows its figures. `at_end`, to be retired once the app shows
`reading`, is the readings the app showed when the
take was saved (`rate_s_per_day`, `amplitude_deg`, `beat_error_ms`
signed, from the unlock, else from the drop), not a new analysis, and `null` for a take saved
without them. The app's figures are on the sound card's clock, so
`at_end` adds `rate_corrected_s_per_day`, that rate corrected from the
take's clock log as `session` reads it, when the take has a clock log
that can be read; the text output and the app's Watches view show it in
place of the rate as heard. A take is `incomplete` when a file its session file lists
is not there, or a folder it lists holds no sound. A NAME that matches no
watch, or a folder with no library in it, exits 1 with one line on
stderr naming it. The text form of `list` gives each watch's calibre,
number of takes, the UTC start of its newest take and its size; `show`
gives the watch's details and each take on a line. Each watch's `id` (in
both documents; `null` for `show Unsorted`) is the random id in its
`watch.toml`: it stays when the watch is renamed, so a program that keeps
its own record of a watch can find it again by it.

### Changing the watches

```sh
timegrapher watches new NAME [--calibre C] [--bph N] [--lift DEG] [--notes TEXT] [--field LABEL=VALUE]...
timegrapher watches edit NAME [--rename NEW] [--calibre C] [--bph N] [--lift DEG] [--notes TEXT] [--field LABEL=VALUE]... [--remove-field LABEL]...
timegrapher watches add-take WATCH FOLDER [--move]
timegrapher watches move-take TAKE --to WATCH
timegrapher watches keep-analysis-only TAKE
timegrapher watches delete-take TAKE --yes
timegrapher watches add-photos WATCH FILE...
timegrapher watches delete-photo PHOTO --yes
```

(each with `[--library DIR] [--json]`). The changes the app's Watches
view makes, each by the same function of core's `library` the app calls,
so a watch or take changed here reads in the app as one changed there.
`new` adds a watch (the first makes the library, in a folder that is
missing or empty); `edit` changes only what it is given, and the folder
keeps its name. `--calibre` takes a name from the calibre table or the
user's own calibres (`calibres --search` finds it; `""` on `edit` sets
none), and the lift angle and beat rate it gives become the watch's, as
in the app's form: `--bph` or `--lift` for a figure the calibre gives is
refused. `--field` sets a field the watch has (its label ignoring case),
or adds it as text. `add-take` copies a take folder (a `session.toml` or
`session.json` with its sound, or `analysis/` for one kept as its
analysis) into a watch's `takes/`, or moves it with `--move`; it is put
together beside its place and moved in whole, so a copy cut short is
never a take, and a folder already in the library is refused.
`move-take` moves a take of the library (its `path` from `show`) to
another watch, filing one from Unsorted. `keep-analysis-only` reads the
take's sound once more, keeps its analysis, checks it reads back and only
then deletes the sound, as Keep Analysis Only does (it takes as long as
reading the take twice; a take whose `session.toml` lists its sound files
by name is refused). `delete-take` and `delete-photo` can't be undone and
need `--yes`.

Every change takes the library's lock, as the app does, so it is refused
while a copy of the app has the library open, saying so: two programs
never write the same watch at once. `add-take` alone works while the app
is open, since it only adds a new folder; the app lists the take when its
window next comes to the front, or at its next start.

`--json` gives schema `timegrapher.watches-change/1`: `action` (the
subcommand), `watch` (`id`, `name`, `folder` after the change; `null`
for a take in Unsorted), `take` (the take's folder after the change; for
`delete-take` the one deleted; else `null`), `photos` (those added or
deleted), `left` (`null`, or what was left behind where the change was
made but part of the original couldn't be deleted, also on stderr) and
`caveats` (empty). A refused change exits 1 with one line on stderr and
changes nothing.

## `excerpt`

```sh
timegrapher excerpt RECORDING --out FILE.flac [--from S] [--to S] [--json]
```

Writes a stretch of a recording as one FLAC file, its samples untouched,
for a program that keeps the sound beside a reading (a shop's records,
say). RECORDING is a sound file or a take folder, whose segments are read
one after another as `session` reads them. `--from` and `--to` are seconds
from the start of the recording, as a session reading's
`measurement.start_s` and `end_s` give them, so the stretch a test read
is `--from` its first reading's `start_s` `--to` its last one's `end_s`.
`--from` defaults to 0 and `--to` to the end; a `--to` past the end stops
at the end. The file is decoded again and checked sample for sample before
it is written (core's `excerpt`), and it is never written over: a FILE
already there exits 1 and is left as it was. A take kept as its analysis
alone has no sound to cut and exits 1. It never opens the microphone.

`--json` prints `timegrapher.excerpt/1`: `out` (FILE as given), `from_s`
and `to_s` (the stretch cut), `duration_s`, `sample_rate`, `channels`,
`bits_per_sample`, `bytes` (the file's size) and `sha256` (its SHA-256,
lowercase hex). `input.files` lists the sound files read and
`input.settings` the `from_s` and `to_s` asked for (`to_s` null when not
given).

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
mathematical and representation constraints, and the lift angle for its one
range (core `input_domain::LIFT_RANGE_DEG`, which the app's lift fields use too):

- `--bph`: a positive `u32` (1 through 4294967295), for `analyze`, `shape`,
  `profile`, `long`, `series`, `doctor`, `record`, `session`, `synth` and `report`.
- `--lift`: from 10 to 90 degrees, for `analyze`, `shape`, `profile`, `long`,
  `series`, `record`, `session` and `report`, as are a session file's `lift`
  and MCP's `lift_deg`.
- `--escape-teeth`: a positive `u32`, for `analyze`, `long` and `series`.
- `--card-ppm` (`analyze`, `long`) and the positional ppm in `clock set`:
  finite, with `1 − ppm × 10⁻⁶ > 0`, so the clock scale advances time and
  the correction denominator is positive. Zero and admissible negative ppm
  remain valid; this does not establish calibration accuracy.
- `--trend-average` (`analyze`): one of the app's averaging times, 2, 4,
  10, 20, 30 or 60 seconds (core `live::AVERAGES`), as is MCP's
  `average_s`.

CLI validation happens during argument parsing, before audio/device access,
output creation or storing a correction. MCP validates exposed measurement BPH
and lift arguments before invoking the CLI; its schema declares the same domains.
Defaults, explicit 52° lift and zero correction keep their existing meanings.
Passing these domain checks does not guarantee every subsequent numerical
operation is representable for extreme inputs or that a watch uses those settings.

This is an explicit-argument contract. Separately, consumed session manifests
validate `bph`, `lift`/`lift_angle`, `escape_teeth` and `card_ppm_fast` against the same
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


### Beat-error aggregate provenance

Source-aware position averages, session state means/spreads, isochronism differences,
and Steadiness beat-error statistics select one source before calculating: unlock
when present, otherwise drop. Each carries `beat_error_source`,
`beat_error_sources: {unlock, drop}`, and factual `caveats`. A
`beat_error_source_readings` caveat records `source`, `used`, and `readings` when
observations of the other source were excluded, and `counted`: `readings`, or
`windows` when the counts are of the amplitude's 2 s windows, as in the
steadiness figures. Reports written before `counted` read as `readings`. Drop-only statistics carry
`beat_error_from_drop`. Minimum counts apply after selection; a difference with
only one selected endpoint is null.

Counts describe the immediate inputs to that statistic: repeated readings for a
position, position means for a session state, and endpoints for an isochronism or
wind-down difference. Steadiness series and wind-down stretches count eligible
fine-window readings before binning; excluded sources leave holes in the grid.
Series-level provenance applies to its spread, trend, cycles and other tests.

Saved FullWind rows now persist `beat_error_source`; absent fields deserialize
as null. Their individual values remain visible without an invented source.
The app uses source-aware aggregates; unknown-source rows are excluded from
pooled beat-error means and spreads, while rate and amplitude stay available.
A `beat_error_unknown_source` caveat carries the number of excluded `readings`,
including when no beat-error aggregate can be given. Source counts continue to
count only known unlock/drop inputs. The temporary `Figures`/`indices` adapter
has been removed.
Raw explicitly named unlock/drop fields retain their meaning. A direct
`Measurement` falling back to the beat-time fit still has no beat-error ±;
no uncertainty from drop windows is attached to that different estimator.
