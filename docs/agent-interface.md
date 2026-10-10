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
  engine moved away from tg ([regress](regress.md)).
- Nothing changes the computer's settings unless the person asked for it:
  `doctor` proposes mixer changes and only makes them with `--apply`.
  An agent must ask the person before passing `--apply`.

## The JSON envelope

Every document starts with the same three fields:

```json
{
  "schema": "timegrapher.analyze/1",
  "software": { "name": "timegrapher", "version": "0.1.0", "os": "linux", "arch": "x86_64" },
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

| Kind | Command | The command's own fields |
|---|---|---|
| `analyze` | `analyze FILE --json` | `duration_s`, `sample_rate`, `bph`, `beats_found`, `overall` (`rate_s_per_day`, `beat_error_ms`, `period_s`, `jitter_us`), `rate_p05`, `rate_p95`, `jitter_us`, `amplitude_deg` (+ even/odd), `lift_deg`, `timing_periods`, `amplitude_periods` |
| `shape` | `shape FILE --json` | per pallet stone (even/odd beats): unlock, impulse and drop timing and level; see [fault-signatures.md](fault-signatures.md) |
| `profile` | `profile FILE --at S --span S --json` | `a` (the tick: even beats) and `b` (the tock: odd beats): `beats`, `t0_ms`, `step_ms`, `median`, `p10`, `p90` (envelope point by point), `floor`, `unlock_ms`, `drop_ms`, `peak_ms`, `amplitude_deg`, `sound1_ms`, `sound2_ms`, `sound3_ms`; times in ms from the beat. `--svg FILE` also draws them |
| `long` | `long FILES --json` | rate and amplitude over the run, clock calibration, periodic components named after wheels with false-alarm probability; also written to `summary.json` beside `report.html`; see [long-runs.md](long-runs.md) |
| `series` | `series FILES --json` | per series (`rate`, `amplitude`, `beat_error`): `verdict` (`steady`, `periodic`, `two_states`, `measurement`, `shifting_mean`, `drifting`, `wandering`, `too_short`), `headline`, and the views behind it (`autocorrelation`, `allan`, `cusum`, `changes`, `trend`, `period`, `two_state`, `cycle`); `findings` (`code`, `series`, `severity`, `title`, `evidence`, `advice`); see [series.md](series.md) |
| `devices` | `devices --json` | `devices[]`: `id`, `name`, `manufacturer`, `is_default`, `default_config`, `supported[]` (`min_channels`, `max_channels`, `min_sample_rate`, `max_sample_rate`, `sample_format`) |
| `doctor` | `doctor --json` | `source`, `check`, `mixer`, `fixes[]`, `applied`, `after`, `verdict` (below) |
| `session` | `session PATHS --json` | per-position `readings`, Witschi's characteristic values and `findings` (`code`, `severity`, `recording`, `title`, `evidence`, `advice`); also written to `summary.json`; see [sessions.md](sessions.md) |

Tick and tock: the engine numbers beats from the first one it hears, so the tick is simply the even beats and the tock the odd ones, as tg's tic and toc are; it cannot tell which pallet stone is which. Fields named `even`/`odd` or `a`/`b` mean tick/tock in that order, and people-facing text says Tick and Tock.

Rates are seconds per day, positive gaining. Rates from a recording are on
the sound card's clock unless `long` was given a clock log; a sound card can
be tens of ppm off (1 ppm is 0.0864 s/d).

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

- `record`: capture to FLAC with the provenance sidecar and a clock log.
- `schema KIND`: print a JSON Schema for each document.
- An MCP server, if live streaming to an agent or use from chat-only apps
  turns out to need it; it would wrap the same core.
