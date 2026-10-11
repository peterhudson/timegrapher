# Command reference

`timegrapher --help` and `timegrapher <command> --help` are always
complete and current; check them when this file and the program disagree.

Commands: `update`, `devices`, `doctor`, `calibres`, `record`, `analyze`, `shape`, `profile`,
`long`, `series`, `session`, `clock`, `report`, `regress`, `synth`. `session` (the
multi-position report) is documented in the repository's
`docs/sessions.md`, `series` in `docs/series.md`. What each finding code
of `doctor`, `session` and `series` means, and what to do about it, is in
[findings.md](findings.md).

**Calibres** (from this version; `timegrapher --help` shows whether a
build has them):

```
timegrapher calibres [NAME] [--json]       the built-in calibre table: vph, lift angle, escape wheel teeth, wheel turn periods, sources
timegrapher analyze FILE --calibre NAME    beat rate, lift angle and wheel names from the table; --bph, --lift, --escape-teeth win
timegrapher session FOLDER --calibre NAME  the same for a session: the flag, then session.toml, then the calibre, then the defaults
```

Take the lift angle from `calibres`, never from memory; a calibre with no
published lift angle shows "-" and `analyze` uses 52°. `session` also
gives, per position, the change in beat error between states of wind.

**Tick and Tock.** When talking to people, call the two sides of the
beat the **Tick** (the even beats) and the **Tock** (the odd beats), never
"A" and "B". The program can't tell which is physically the tick (that
depends on where in the swing the recording started), so Tick simply means
the even beats. Machine fields keep their own names: `even`/`odd` in
`analyze` and `shape`, `a`/`b` in `profile`, where `a` is the Tick and `b`
the Tock.

Exit codes: **0** success, **1** error (message on stderr), **3** `doctor`
found a problem, `record` made a take not fit to keep, or `regress`
found the engine moved away from tg.

Input files: WAV or FLAC, any sample rate (44.1 or 48 kHz recommended),
mono or the first channel.

## The JSON envelope

Every `--json` document is one JSON object with these top-level fields,
then the command's own fields alongside them:

```json
{
  "schema": "timegrapher.analyze/1",
  "software": { "name": "timegrapher", "version": "0.1.0", "os": "linux", "arch": "x86_64" },
  "input": {
    "files": [ { "path": "dialup.wav", "recording": { "...": "from dialup.wav.json if present" } } ],
    "settings": { "bph": null, "lift_deg": 52.0, "notch_hz": [], "highpass_hz": 1500.0, "escape_teeth": 15 }
  },
  "duration_s": 120.0
}
```

- `schema` is `timegrapher.<kind>/<version>`: `analyze`, `shape`,
  `profile`, `long`, `session`, `doctor`, `devices` (and `recording` for the sidecar `doctor --save`
  writes). Within a version fields are only added; check the kind and
  version before reading, and ignore fields you don't know.
- `input.files[].recording`: when a recording has a sidecar `FILE.json`
  (written by `doctor --save`), its device and mixer settings come along,
  so you can see how it was recorded.
- `input.settings`: the options the command ran with. `bph: null` means
  it was guessed; the guessed value is in the body.

The interface is new (being added in the same change as this skill);
field names below follow the code at the time of writing.

## devices

```
timegrapher devices [--json]
```

Lists sound inputs. JSON body: `devices`: list of
`{ id, name, manufacturer, is_default, default_config, supported }`,
where each config is `{ min_channels, max_channels, min_sample_rate, max_sample_rate,
sample_format }`. `input.host` names the audio system (ALSA, CoreAudio,
WASAPI).

Needs a build with the default `live` feature.

## doctor

```
timegrapher doctor [--device NAME] [--card N] [--seconds S] [--file WAV]
                   [--save WAV] [--apply] [--bph N] [--json]
```

| Option | Meaning |
| --- | --- |
| `--device NAME` | Input to use: part of its name or its id from `devices` (default: system default input) |
| `--card N` | Linux: ALSA card number or name whose mixer to read (default: from the device, or the only USB card) |
| `--seconds S` | How long to listen (default 5) |
| `--file WAV` | Check a recording instead of listening |
| `--save WAV` | Keep what it heard, with `WAV.json` beside it (device, format, mixer settings) |
| `--apply` | Run the proposed mixer commands (Linux) and check again. **Only after the person agrees.** |
| `--bph N` | Beat rate, if the guess is wrong |

JSON body:

| Field | Meaning |
| --- | --- |
| `source` | The device and format recorded (or `{ file }`) |
| `check` | The measurements (below) |
| `mixer` | Linux: `{ card, controls: [{ name, index, capture: [cur, min, max], capture_db, switch }] }` |
| `fixes` | `[{ issue, description, command?, requires_consent }]`. `command` is an argument list (Linux `amixer`); without one, `description` is instructions for the person |
| `applied` | Whether `--apply` ran the commands |
| `after` | The check repeated after applying (live only) |
| `verdict` | `ok` or `needs_attention` |

`check` fields: `sample_rate`, `duration_s`, `peak_dbfs`, `rms_dbfs`,
`dc_offset`, `clipped_samples`, `bph`, `beats_expected`, `beats_found`,
`tick_level_dbfs`, `noise_level_dbfs`, `tick_to_noise_db`, `signal_x`
and `signal_band` (the desktop app's "signal N×" and its band: `good`,
`fair`, `poor`, `none`; see the table in setup-and-microphone.md), `gap_rise_db`
(background rise between ticks; several dB means automatic gain),
`rate_s_per_day`, `beat_error_ms` (a few seconds only, uncalibrated),
`suggested_gain_change_db` (negative: turn down), and `issues`:
`[{ code, severity, title, evidence, advice }]` with `severity` `fault` or `warning` and
`code` one of `silent`, `too_quiet`, `clipping`, `hot`, `agc_suspected`,
`no_ticks`, `noisy`. What each means and how to fix it:
[setup-and-microphone.md](setup-and-microphone.md#checking-the-signal-with-doctor).

## update

```
timegrapher update [--json]
```

Asks GitHub once whether a newer release is published. JSON body
(`timegrapher.update/1`): `current`, `latest` (`version`, `url` of the
release page, `published_at`, `notes`) and `update_available`. It sends no
account or identifier, and downloads and installs nothing; the person
updates from `latest.url`. Exit 1 with the reason on stderr when GitHub
can't be reached, is limiting requests, or has no release. Builds without
the `update-check` feature say they have no network code.

## record

```
timegrapher record FOLDER --position P [--seconds 120] [--device NAME]
                   [--watch TEXT] [--calibre NAME] [--bph N] [--lift DEG]
                   [--wind-h H] [--file AUDIO] [--json]
```

Records one position for `--seconds`, then stops; it listens only while
it runs. Each run makes a take folder in FOLDER as the desktop app saves
one (`audio-001.wav`, `clock.csv`, `session.json`, `session.toml`),
checks it, and when a watch is heard and under 1% of beats clip, adds it
to `FOLDER/session.toml` (written on the first take with the watch,
calibre, bph, lift and `settle_s = 30`) with its position and `--wind-h`.
`timegrapher session FOLDER` then reads them all.

JSON body: `take`, `session_file` (null when not added), `position`,
`wind_h`, `seconds`, `source`, `sample_rate`, `check` (`watch_heard`,
`bph`, `beats_found`, `beats_expected`, `rate_s_per_day`,
`amplitude_deg`, `beat_error_ms`, `clipping`), `verdict` (`ok`,
`no_watch_heard`, `clipping`). Exit 3 when the verdict is not `ok`:
record that position again. `--bph` or `--lift` that disagree with the
folder's session file are an error. `--file` reads a recording instead of
the microphone.

## analyze

```
timegrapher analyze FILE [--bph N] [--lift DEG] [--notch HZ,HZ,...]
                    [--highpass HZ] [--escape-teeth N]
                    [--beats CSV] [--windows CSV] [--json]
```

| Option | Default | Meaning |
| --- | --- | --- |
| `--bph` | guessed | Beat rate. Guessed from 12,000, 14,400, 17,280, 18,000, 19,800, 21,600, 25,200, 28,800, 36,000, 43,200, 72,000 |
| `--lift` | 52 | Lift angle, degrees; amplitude scales with it |
| `--notch` | none | Steady tones to remove, Hz (mains hum, whines) |
| `--highpass` | 1500 | High-pass corner, Hz |
| `--escape-teeth` | 15 | For naming the escape wheel's period |
| `--beats` | | CSV, one row per beat: `index,time_s,quality,residual_us` |
| `--windows` | | CSV of rate (10 s windows) and amplitude (2 s windows): `kind,start_s,end_s,rate_s_per_day,beat_error_ms,amplitude_even_deg,amplitude_odd_deg` |

JSON body:

| Field | Meaning |
| --- | --- |
| `duration_s`, `sample_rate` | The recording |
| `bph` | Beat rate used |
| `beats_found` | Ticks found; compare with `duration_s × bph / 3600` |
| `watch_heard` | False when the beats are no steadier than noise (an empty stand, a stopped watch): every other number then describes noise; say no watch was heard and show none of them |
| `faults[]` | Possible escapement faults from the shape of the Tick and Tock (from this version): `id`, `label`, `side`, `strength` (all `possible` for now), `evidence`, `see`. Empty when nothing is seen or the shape can't be judged. How to read and report each one: [findings.md](findings.md#faults-the-program-names) |
| `overall.rate_s_per_day` | Rate over the whole file, + gains. **Uncalibrated**: the sound card's clock error (1–4 s/d typical) is included |
| `beat_error_unlock_ms` | **The beat error to quote**: signed, measured from the unlock, as tg and commercial timegraphers measure it; report its absolute value |
| `overall.beat_error_ms` | Signed beat error from the fit, measured from the drop (positive: even beats late); differs from the unlock figure when Tick and Tock take different times from unlock to drop |
| `overall.period_s` | Fitted beat period |
| `overall.jitter_us`, `jitter_us` | Scatter of single beats, microseconds (overall, and the median within 10 s windows) |
| `overall.beats_used` | Beats in the fit |
| `rate_p05`, `rate_p95` | 5th and 95th percentile of the rate in 10 s windows: how much the trace wanders |
| `amplitude_deg` | Median amplitude, degrees |
| `amplitude_even_deg`, `amplitude_odd_deg` | Per side (the two pallet stones) |
| `lift_deg` | Lift angle used |
| `timing_periods`, `amplitude_periods` | Up to five periodic components: `{ period_s, power, prominence, wheel }`. `power` is the fraction of variance explained; `wheel` is a wheel within 6% of the period. A hint only on a short file: confirm with `long` |

`overall` is null when there weren't enough clean beats to fit.
Amplitudes outside 100–380° are discarded as implausible and come back
null.

## shape

```
timegrapher shape FILE [--bph N] [--lift DEG] [--notch HZ,...] [--highpass HZ]
                  [--templates CSV] [--json]
```

`--templates` writes the averaged even and odd tick, `time_ms,even,odd`.
How to read it: [tick-shape.md](tick-shape.md).

JSON body: `even` and `odd` (one per side), `windows` (each 2 s window's
`{ start_s, end_s, even, odd }` shapes), `template_start_ms`.

Each side: `windows` (median of each measure over the 2 s windows: the
better reading of timing), `whole` (measured on the whole-recording
average: best for faint extra events), `windows_measured`,
`windows_with_1`, `windows_with_2`, `windows_with_extra_pre`,
`windows_with_extra_post`, and `spread` `{ beats, i13_ms, i13_sd_us }`
(beat-to-beat spread of the 1-to-3 interval).

Each shape (times in ms from the unlock edge; levels relative to the
template's floor):

| Field | Meaning |
| --- | --- |
| `unlock_to_drop_ms` | What amplitude is computed from |
| `unlock_at_ms` | Unlock edge from the beat's reference point |
| `t1_ms`, `t2_ms`, `t3_ms` | Each sound halfway up its rise (null if not found) |
| `peak1_ms`, `peak2_ms`, `peak3_ms` | Each sound's peak |
| `i12_ms`, `i23_ms`, `i13_ms` | Intervals between sounds |
| `level1`, `level2`, `level3`, `ratio13`, `ratio23` | Sound levels and their ratios to the drop |
| `valley12`, `valley23` | Lowest point between two sounds against the quieter: 0 well separated, 1 no dip |
| `fill12`, `fill23` | Mean level between two sounds against their mean |
| `tail_ratio` | Mean level 3–15 ms after the drop peak against the drop |
| `noise_ratio` | Noise between beats against the silence before the unlock |
| `rises` | Rises from unlock to drop, counting the drop: 3 is textbook |
| `extra_pre`, `extra_post` | Extra sounds before 1 / after 3: `[{ t_ms, level }]` |

## profile

```
timegrapher profile FILE [--at S] [--span S] [--bph N] [--lift DEG]
                    [--notch HZ,...] [--highpass HZ] [--json] [--svg FILE]
```

The median sound of the Tick and of the Tock over a stretch of the
recording (`--at` seconds in, `--span` seconds long; defaults 0 and 10),
with the marks the engine measured on it: the unlock, the drop and its
peak, and the three sounds. **Use it when amplitude or beat error look
wrong** (implausibly high or low, the Tick and the Tock amplitudes far
apart, a beat error that doesn't match what you hear): it shows which
sound the engine took as the unlock and which as the drop. A movement in
a plastic holder, a case that rings, or a quiet first sound can put a mark
on the wrong sound, and the amplitude follows it. `--svg` draws both
envelopes with the marks, for a person to look at. Pick a quiet stretch
(`--at`) away from handling noise.

JSON body: `a` (the Tick) and `b` (the Tock), each with:

| Field | Meaning |
| --- | --- |
| `beats` | Beats averaged in the stretch |
| `t0_ms`, `step_ms` | Time of the first envelope point and the spacing; all times in ms from the beat |
| `median`, `p10`, `p90` | The envelope point by point: median, and 10th and 90th percentiles across beats (a wide band means the sound varies from beat to beat) |
| `floor` | Background level |
| `unlock_ms`, `drop_ms`, `peak_ms` | The engine's marks: amplitude comes from unlock to drop |
| `sound1_ms`, `sound2_ms`, `sound3_ms` | The three sounds (unlock, impulse, drop) |
| `amplitude_deg` | Amplitude from these marks at `--lift` |

Read it as: the unlock mark should sit at the start of the first small
sound, not on noise before it or on the second sound; the drop on the
start of the big last sound. If the Tick and the Tock marks differ by much
more than their sounds do, the engine has picked different sounds on the
two sides; report that rather than the amplitude.

## long

```
timegrapher long FILES... [--clock LOG] [--bph N] [--lift DEG] [--notch HZ,...]
                 [--highpass HZ] [--escape-teeth N] [--wheel "NAME=SECONDS"]...
                 [--out DIR] [--json]
```

`FILES` are read as one continuous recording in the order given; a folder
stands for its WAV and FLAC files in name order. `--clock` is the
sound-card calibration log; `--wheel` names another wheel by its period
(repeatable). Output folder (default `<recording>_long/` beside it):
`report.html`, `summary.json` (the same as `--json`), `beats.csv`
(`index,time_s,true_time_s,quality`), `amplitude.csv`
(`start_s,end_s,true_start_s,even_deg,odd_deg`), `slices.csv`
(`start_s,end_s,rate_s_per_day,beat_error_ms,amplitude_deg`), `folds.csv`
(`series,period_s,phase,mean,shape,unit`). Progress goes to stderr.

JSON body:

| Field | Meaning |
| --- | --- |
| `duration_s`, `sample_rate`, `bph`, `lift_deg` | The recording and settings |
| `beats_found`, `clean_fraction` | Beats, and the fraction matched well |
| `clock` | Null, or `{ points, rejected, span_s, ppm, rate_error_s_per_day, residual_ms, tracks_drift }`. `ppm` positive: sound card slow, uncorrected rates read fast by `rate_error_s_per_day` |
| `overall` | As for `analyze` (rate corrected when `clock` is given) |
| `rate_p05`, `rate_p95` | Spread of the rate over slices of about 1/400 of the run |
| `amplitude_deg`, `amplitude_p05`, `amplitude_p95` | Median and spread |
| `rate_periods`, `amplitude_periods` | Components found: `{ period_s, resolution_s, significance, harmonics, explained, peak_to_peak, wheel, nearest_wheel }` |

Component fields: `significance` is −log10 of the false-alarm chance (2 =
1%, 65 = 1e-65); `harmonics` 1 for a smooth wave, up to 8 for a short
sharp event; `explained` the fraction of the detrended variation;
`peak_to_peak` in s/d (rate) or degrees (amplitude); `wheel` the matching
wheel's name or null; `nearest_wheel` `[name, percent_off]`. How to read
it: [long-runs.md](long-runs.md).

## series

```
timegrapher series FILES... [--clock LOG] [--bph N] [--lift DEG]
                   [--calibre NAME] [--reading S] [--json]
```

Is the take steady? Reads rate, amplitude and beat error in time order
(readings of `--reading` seconds, default 10) and gives each a `verdict`
(`steady`, `periodic`, `two_states`, `measurement`, `shifting_mean`,
`drifting`, `wandering`, `too_short`) and a plain `headline`, plus
`rate_amplitude` (does the rate follow the amplitude) and `findings[]`.
`--calibre` names periodic changes after that movement's wheels; an
unknown name lists the calibres the program knows. Same input as `long`.
What each verdict means for the watch: [findings.md](findings.md#series-long-runs).

## report

```
timegrapher report [RECORDING...] [--at S] [--span S] [--no-audio]
                   [--kind bug|reading|setup|idea] [--note TEXT]
                   [--bph N] [--lift DEG] [--out DIR]
                   [--listened-back] [--cc0] [--json]
timegrapher report --replay BUNDLE [--json]
```

Writes `timegrapher-report-<utc>.zip` for an issue. It never opens the
microphone and sends nothing anywhere. The zip holds:
- `manifest.json` (`timegrapher.report/1`: software with git hash, OS,
  `kind`, `note`, the commands run, every file's SHA-256, the `consent`
  flags, what was `redacted` and what was `left_out`);
- `audio/excerpt.flac`: `--span` seconds (default 300) from `--at`, as a
  lossless cut, with any `doctor --save` sidecar;
- `results/analyze-excerpt.json`, plus `analyze-whole.json` or
  `long-whole.json`;
- a folder's clock log;
- `env/clock.json`;
- `logs/`.

The home folder becomes `~`, and the user and host names become `<user>`
and `<host>`. `--listened-back` and `--cc0` are the person's own consent:
never pass them for them. `--kind` defaults to `reading` with a recording
and `bug` without. `--json` prints the manifest.

`--replay` checks the bundle's hashes, reruns the excerpt's `analyze` with
this build, and marks each command `Same`, `Moved` (with each value then and
now) or `Not run`. `--json` gives `timegrapher.replay/1`. Reference: the
repository's `docs/reports.md`.

## synth

```
timegrapher synth OUT.wav [--duration S] [--bph N] [--amplitude DEG] [--rate S/D]
                  [--beat-error MS] [--snr DB] [--fault-period S]
                  [--fault-length S] [--fault-rate S/D] [--fault-amplitude DEG]
```

Writes a synthetic recording with known values (defaults: 60 s, 28,800
bph, 270°, +5 s/d, 0.4 ms, 30 dB). `--fault-period 60` adds a fault that
repeats like a bad tooth: for `--fault-length` seconds (default 8) of each
period, the rate changes by `--fault-rate` (default −30 s/d) and the
amplitude by `--fault-amplitude` (default −15°). Use it to show someone
what a fault looks like, or to check the program works before blaming
the watch.
