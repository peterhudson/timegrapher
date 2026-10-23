# Command reference

`timegrapher --help` and `timegrapher <command> --help` are always
complete and current; check them when this file and the program disagree.

Commands: `update`, `devices`, `doctor`, `calibres`, `watches`, `record`,
`analyze`, `shape`, `profile`, `long`, `series`, `session`, `clock`,
`report`, `regress`, `synth`. `session` (the
multi-position report) is documented in the repository's
`docs/sessions.md`, `series` in `docs/series.md`. How to read what
`doctor`, `session` and `series` measure, and what to do about it, is in
[findings.md](findings.md). The program states what it measured and
gives no verdict, grade, severity, advice or fault name (schema version
2 of `analyze`, `doctor`, `record`, `series` and `session` removed
them). docs/agent-interface.md's row for each command is the
authoritative list of fields; this page covers what an agent reads.

**Calibres** (from this version; `timegrapher --help` shows whether a
build has them):

```
timegrapher calibres [NAME] [--json]       the built-in calibre table: bph, lift angle, escape wheel teeth, wheel turn periods, sources
timegrapher calibres --search QUERY        the calibres that answer QUERY, best first (a name, another name or the maker: "dd 3235", "AP 3120")
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

Exit codes: **0** success, **1** error (message on stderr), **3**
`regress` found the engine moved away from tg.

Input files: WAV or FLAC, any sample rate (44.1 or 48 kHz recommended),
mono or the first channel.

## The JSON envelope

Every `--json` document is one JSON object with these top-level fields,
then the command's own fields alongside them:

```json
{
  "schema": "timegrapher.analyze/2",
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

`check` fields: `sample_rate`, `duration_s`, `peak_dbfs`, `rms_dbfs`,
`dc_offset`, `clipped_samples`, `bph`, `beats_expected`, `beats_found`,
`tick_level_dbfs`, `noise_level_dbfs`, `tick_to_noise_db`, `signal_x`
(read it against the table in setup-and-microphone.md), `gap_rise_db`
(background rise between ticks; several dB means automatic gain),
`rate_s_per_day`, `beat_error_ms` (a few seconds only, uncalibrated),
`suggested_gain_change_db` (negative: turn down), and `issues`:
`[{ code, title, evidence }]` with `code` one of `silent`, `too_quiet`,
`clipping`, `hot`, `agc_suspected`, `no_ticks` (`doctor/2`). What each
means and how to fix it:
[setup-and-microphone.md](setup-and-microphone.md#checking-the-signal-with-doctor).

## update

```
timegrapher update [--json]
```

Asks GitHub once whether a newer release is published. JSON body
(`timegrapher.update/1`): `current`, `latest` (`version`, `url` of the
release page, `published_at`, `notes`) and `update_available`. It sends no
account or identifier, and downloads and installs nothing; the person
updates from `latest.url`. With no release published yet it says
so and exits 0, with `latest` null and `update_available` false. Exit 1
with the reason on stderr when GitHub can't be reached or is limiting
requests. Builds without
the `update-check` feature say they have no network code.

## watches

```
timegrapher watches list [--all] [--library DIR] [--json]
timegrapher watches show NAME [--library DIR] [--json]
```

The library the desktop app keeps its watches and their takes in:
`Timegrapher Data` in the home folder, or the folder `--library` names.
It only reads, so it can run while the app is open. `list`
(`watches/1`) gives each watch with its calibre, number of takes,
newest take and size, and how many takes wait in Unsorted; archived
watches only with `--all`. `show NAME` (`watch/1`) gives one watch's
details and every take, newest first, found by its name ignoring case;
`show Unsorted` gives the takes Quick Listen made. A field the user
never set is `null`. Each take's `path` is a take folder to pass to
`session`, `long` or `series`; `audio` false means the take kept only
its analysis, so it can't be measured again; `at_end` is the readings
the app showed when the take was saved, not a new analysis. A NAME that
matches no watch exits 1. Through the MCP server: `list_watches` and
`show_watch`.

## record

```
timegrapher record FOLDER --position P [--seconds 120] [--device NAME]
                   [--watch TEXT] [--calibre NAME] [--bph N] [--lift DEG]
                   [--wind-h H] [--file AUDIO] [--power-reserve] [--json]
timegrapher record --watch NAME [--library DIR] --position P [...]
```

Records one position for `--seconds`, then stops; it listens only while
it runs. Each run makes a take folder in FOLDER as the desktop app saves
one (`audio-001.wav`, `clock.csv`, `session.json`, `session.toml`),
checks it, and adds every take to `FOLDER/session.toml` (written on the
first take with the watch, calibre, bph, lift and `settle_s = 30`) with
its position and `--wind-h`, and a comment line for no regular beat or
clipping. `timegrapher session FOLDER` then reads them all.

Without a FOLDER, `--watch NAME` names a watch in the desktop app's
library (see `watches`): the session folder is made among that watch's
takes, named by the local time it started (`2026-10-22 09.15`), and the
watch's calibre, bph and lift fill in those not given. Give that new
folder (the one `session_file` is in) as FOLDER to record the next
position into the same test. Recording into the library is refused
(exit 1, before anything is made) while the app has the library open,
and so is a FOLDER that is one of a library watch's takes. With a
FOLDER, `--watch` is the owner's own words for the watch. The MCP
server's `record_position` always takes a folder.

`--power-reserve`, in place of `--seconds`, makes the run a Power
Reserve Test: it records in the one position until the watch stops (the
last beat heard before 10 minutes with none) or 80 hours pass, and
writes the run, hour by hour, to the session folder's
`power-reserve.json` (`ending`: `stopped`, `by_hand` or `limit`;
`run_s` in true seconds; the silences of 30 s or more; `hours[]`, each
measured as `session` measures a stretch). Without a FOLDER the folder
is named by the date and time and "Power Reserve". The `--json` document
adds `power_reserve` with the same contents, and `session FOLDER --json`
gives it too whenever the folder holds `power-reserve.json`.

JSON body (`record/2`): `take`, `library_watch` (null when FOLDER was
given), `session_file`, `position`, `wind_h`,
`seconds`, `source`, `sample_rate`, `check` (`watch_heard`,
`timing_scatter_ms`, `timing_scatter_fraction`, `bph`, `beats_found`,
`beats_expected`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms`,
`clipping`). When no regular beat was found or the ticks clipped, record
that position again. `--bph` or `--lift` that disagree with the
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
| `--escape-teeth` | the calibre's, or 15 | Escape wheel teeth. With no calibre, the escape wheel is named only when this is given |
| `--beats` | | CSV, one row per beat: `index,time_s,quality,residual_us` |
| `--windows` | | CSV of rate (10 s windows) and amplitude (2 s windows): `kind,start_s,end_s,rate_s_per_day,beat_error_ms,beat_error_unlock_ms,amplitude_even_deg,amplitude_odd_deg,unlock_even_ms,unlock_odd_ms,drop_even_ms,drop_odd_ms` (the last four: where the unlock and the drop fall on each side) |

JSON body:

| Field | Meaning |
| --- | --- |
| `duration_s`, `sample_rate` | The recording |
| `bph` | Beat rate used |
| `beats_found` | Ticks found; compare with `duration_s × bph / 3600` |
| `watch_heard` | False when the beats are no steadier than noise (an empty stand, a stopped watch): every other number then describes noise; say no watch was heard and show none of them |
| `caveats[]` | What limits how far the readings can be trusted, each with a `code`: `no_watch`, `set_rate_disagrees` (`heard`), `short_span` (`span_s`), `clipped` (`fraction`), `beats_missing` (`fraction`), `lift_unknown`, `clock_unknown`, `breaks` (`count`), `sides_by_count` (`count`), `beat_error_as_size`; a `session` reading's measurement can also carry `masked` (`seconds`). Empty when nothing does; what each means and what to do is in [findings.md](findings.md#what-limits-the-readings-caveats) |
| `beats_found_fraction`, `timing_scatter_fraction` | Beats found over the beats the length holds; the median timing scatter as a share of the beat (0.02 or more: no regular beat) |
| `confidence` | `rate`, `amplitude`, `beat_error`, each `se`: one standard error in the reading's unit. Null when no regular beat is found |
| `shape` | The Tick (`even`) and the Tock (`odd`), each `windows_measured`, `share_with_sound_1`, `share_with_sound_2`, `i12_ms`, `i13_ms`, `ratio13`, `tail_ratio`; and `beat_error_first_quarter_ms`, `beat_error_last_quarter_ms`, `beat_error_quarter_windows`. Read with [tick-shape.md](tick-shape.md#rules-for-possible-faults). Null when no regular beat is found |
| `overall.rate_s_per_day` | Rate over the whole file, + gains. **Uncalibrated**: the sound card's clock error (1–4 s/d typical) is included |
| `beat_error_unlock_ms` | **The beat error to quote**: signed, measured from the unlock (this project's headline beat error); report its absolute value |
| `overall.beat_error_ms` | Signed beat error from the fit, measured from the drop (positive: even beats late); differs from the unlock figure when Tick and Tock take different times from unlock to drop. Across a break (in a `session` reading, also across a masked stretch) where the sign turns over, both beat errors are given as positive sizes, and the caveat `beat_error_as_size` says so: name no side from the sign then |
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
timegrapher long FILES... [--clock LOG] [--device NAME] [--card-ppm PPM]
                 [--bph N] [--lift DEG] [--notch HZ,...]
                 [--highpass HZ] [--escape-teeth N] [--wheel "NAME=SECONDS"]...
                 [--calibre NAME] [--out DIR] [--json]
```

`FILES` are read as one continuous recording in the order given; a folder
stands for its WAV and FLAC files in name order. `--clock` is the
sound-card calibration log. Without one, `--card-ppm` corrects for a
card that many ppm slow (negative if fast); else `--device` uses the
clock error stored for that input, and is an error when none is stored;
else the error stored for the input the recording names, else the
recording's USB lines; with none of these the rates are on the sound
card's clock, uncorrected. `--wheel` names another wheel by its period
(repeatable); `--calibre` names periodic changes after that movement's
wheels. With no calibre, no wheel is named but `--escape-teeth`'s and
`--wheel`'s, and `input.settings.reference_lines` gives 1 min and 1 h
(`label`, `period_s`) as periods to mark, not wheels. The lift angle is
`--lift`, else the calibre's when `--calibre` is given, else 52°. Output folder (default `<recording>_long/` beside it):
`report.html`, `summary.json` (the same as `--json`), `beats.csv`
(`index,time_s,true_time_s,quality`), `amplitude.csv`
(`start_s,end_s,true_start_s,even_deg,odd_deg,beat_error_ms,beat_error_unlock_ms`),
`slices.csv`
(`start_s,end_s,rate_s_per_day,beat_error_ms,beat_error_unlock_ms,amplitude_deg`),
`folds.csv`
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
| `caveats[]` | `sides_by_count` (`count`) and `beat_error_as_size` (the take's beat errors and the slices' are sizes, positive, not signed) as for `analyze`; empty when none. The printed summary's `Sides` and `Conditions` lines say the same |
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
                   [--calibre NAME] [--reading S] [--views] [--maps] [--json]
```

Is the take steady? Reads rate, amplitude and beat error in time order
(readings of `--reading` seconds, default 10) and gives each a
`headline` (what the tests detected, as facts with their statistics),
`unlock_hopping`, and the tests behind it, plus `rate_amplitude` (the
rate against the amplitude, always given) and `findings[]` (`code`,
`series`, `title`, `evidence`) (`series/2`).
`--calibre` names periodic changes after that movement's wheels (with
no calibre, only `--escape-teeth`'s and `--wheel`'s, as for `long`); a
name not in the table is an error that points to `timegrapher calibres
--search`, as it is for `analyze` and `record`. The lift angle is
`--lift`, else the calibre's when `--calibre` is given, else 52°.
`FILES` are read as for `long`. The clock is `--clock`, else the error
stored for the input the recording names, else the recording's USB
lines, else none (`series` has no `--card-ppm` or `--device`). The beat
error series is the unlock's; where the drop fit's sign turns over
across a break, its readings, and so its median and the tests on them,
are sizes, given as positive. `caveats[]` holds `sides_by_count`
(`count`) and `beat_error_as_size` as for `analyze`, and printed `Sides`
and `Conditions` lines say the same. `--views` (with `--json`) adds the
chart data the app's Steadiness pane draws; `--maps` (with `--json`)
adds `time_frequency[]`, an STFT and a synchrosqueezed wavelet map of
each series against time (about 1.6 MB on a 2 h take; the repository's
`docs/series.md`).
How to read the statistics: [findings.md](findings.md#series-and-long-is-it-steady).

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
timegrapher synth --scenario NAME FOLDER [--seed N]
```

Writes a synthetic recording with known values (defaults: 60 s, 28,800
bph, 270°, +5 s/d, 0.4 ms, 30 dB). `--fault-period 60` adds a fault that
repeats like a bad tooth: for `--fault-length` seconds (default 8) of each
period, the rate changes by `--fault-rate` (default −30 s/d) and the
amplitude by `--fault-amplitude` (default −15°). Use it to show someone
what a fault looks like, or to check the program works before blaming
the watch.

`--scenario NAME FOLDER` writes instead a two-hour take folder with one
known variation hidden in it, or none for `control`: twelve 10-minute
FLAC files, `session.json`, and a README saying what is hidden and what
the analysis should find. The app, `series` and `long` open it like a
recording. `--scenario list` names them; `--seed` (default 1) changes
the noise and the beat-to-beat variation drawn (their sizes stay the
same), and the same command always writes the same sound. Reference:
the repository's `docs/synthetic-takes.md`.
