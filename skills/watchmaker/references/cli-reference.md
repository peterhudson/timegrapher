# Command reference

`timegrapher --help` and `timegrapher <command> --help` are always
complete and current; check them when this file and the program disagree.

Commands: `devices`, `doctor`, `analyze`, `shape`, `long`, `synth`. A
multi-position `session` / `report` command is being built and isn't
available yet.

Exit codes: **0** success, **1** error (message on stderr), **3** `doctor`
found a problem.

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

- `schema` is `timegrapher.<kind>/<version>`: `analyze`, `shape`, `long`,
  `doctor`, `devices` (and `recording` for the sidecar `doctor --save`
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
`tick_level_dbfs`, `noise_level_dbfs`, `tick_to_noise_db`, `gap_rise_db`
(background rise between ticks; several dB means automatic gain),
`rate_s_per_day`, `beat_error_ms` (a few seconds only, uncalibrated),
`suggested_gain_change_db` (negative: turn down), and `issues`:
`[{ code, severity, title, evidence, advice }]` with `severity` `fault` or `warning` and
`code` one of `silent`, `too_quiet`, `clipping`, `hot`, `agc_suspected`,
`no_ticks`, `noisy`. What each means and how to fix it:
[setup-and-microphone.md](setup-and-microphone.md#checking-the-signal-with-doctor).

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
| `overall.rate_s_per_day` | Rate over the whole file, + gains. **Uncalibrated**: the sound card's clock error (1–4 s/d typical) is included |
| `overall.beat_error_ms` | **Signed** beat error (positive: even beats late); report its absolute value |
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
