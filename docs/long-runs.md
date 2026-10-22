# Long runs

`timegrapher long` analyses a recording of hours or days: rate and
amplitude over the whole run, and any change that repeats, named after
the wheel whose turn it matches.

```sh
timegrapher long run.flac --clock run-clock.csv --lift 52
```

On a synthetic 30-minute recording whose watch loses 30 s/d and 15° of
amplitude for 8 s of every minute
(`timegrapher synth syn.wav --duration 1800 --rate 8 --snr 24 --fault-period 60`):

```
Recording    30:00 at 48000 Hz, 28800 bph, 14399 beats (100.0% clean)
Clock        not calibrated: rates are read on the sound card's clock, whose error is not measured here (--clock, or a card error stored with `timegrapher clock`, corrects it)
Rate         +4.00 s/d over the run; 10 s slices from -19.1 to 8.1 s/d (5th-95th pct)
Beat error   0.40 ms (from the unlock; 0.40 ms from the drop)
Amplitude    269 deg median; slices from 255 to 270 deg (lift angle 52 deg)
Periodic changes in rate:
      60.00 s ±2.00   swing 36.8 s/d p-p     explains 100%  false alarm <1e-300  matches the fourth wheel (+0.0% off; a match is within ±2.0 s or 1%)
Periodic changes in amplitude:
      20.01 s ±0.22   swing 0.3 deg p-p      explains  23%  false alarm <1e-300  no wheel listed within ±0.2 s or 1%; nearest is the fourth wheel, -66.6% off
      60.03 s ±2.00   swing 11.4 deg p-p     explains  97%  false alarm <1e-300  matches the fourth wheel (+0.0% off; a match is within ±2.0 s or 1%)
Report       syn_long/report.html
```

The rate swing reads high (36.8 against 30 s/d) because the average cycle
is kept to its first eight harmonics, which overshoot on a sharp step.
The amplitude is read in 2 s windows, so its search stops at 16 s (eight
readings a cycle), and a cycle's score counts only its harmonics down to
there, in sets of 1, 2, 4 or 8. The 60 s cycle's fourth harmonic, at
15 s, is under that floor, so it scores on two (60 and 30 s), and its
third harmonic, at 20 s, scored on its own, scores higher. The 20 s line
is found and taken out first, and the 60 s cycle is read from what is
left with its average cycle kept to two harmonics, so its swing reads low
(11.4 against 15°).

The recording is read in 47 s chunks, so memory use does not grow with
its length; a 30-minute file takes about 7 s on one core. Results go to a
folder next to the recording (`--out` to choose):

| File | Contents |
| --- | --- |
| `report.html` | The report: rate and amplitude over the run, the period searches, and for each component found its average cycle and a raster with one row per cycle. Self-contained; open it in a browser. |
| `summary.json` | The figures in the printed summary. |
| `beats.csv` | Every beat: number, time on the sound card's clock, calibrated time, match quality. Re-analysis needs only this and `amplitude.csv`. |
| `amplitude.csv` | Amplitude of the Tick and the Tock (columns `even_deg` and `odd_deg`), and beat error from the drop and from the unlock, in 2 s windows. |
| `slices.csv` | Rate, beat error (from the drop and from the unlock) and amplitude in slices of about 1/400 of the run. |
| `folds.csv` | The average cycle of each component found. |

A capture split into segments with no gaps is read as one recording by
listing the files in order (`timegrapher long seg01.flac seg02.flac ...`)
or by giving the folder that holds them. A folder uses the same order as the
desktop: existing direct WAV/FLAC files listed in its valid `session.toml` come
first (each physical file once), followed by unlisted files in natural name
order (`part2` before `part10`). Missing, nested and whole-folder `.` manifest
entries do not rank files. Without a readable valid manifest, folder expansion
uses natural order; session-loading validation still applies when running a
session itself. Only regular files with WAV/FLAC extensions, ignoring case,
are selected. Explicit command-line paths retain their order and repetitions;
each folder expands at its argument position. The same expansion is used by
`series`, session folder readings and report excerpts. This chooses file order
only, without importing manifest measurement settings into `long` or `series`.

Options as for `analyze` (`--bph`, `--lift`, `--notch`, `--highpass`),
plus three that name the periodic components after wheels:

- `--calibre NAME` takes the train from the table of common calibres
  (`crates/timegrapher-core/data/train-wheels.json`): "ETA 2824-2",
  "Rolex 3235", "NH35" and so on. A name not in the catalogue is an error
  that points to `timegrapher calibres --search NAME`.
  It also sets the lift angle when `--lift` is not given (52° when the
  table has no lift angle for the calibre); it does not set the beat
  rate, which `--bph` gives or the recording shows.
- `--escape-teeth N` sets the escape wheel's teeth. With a calibre whose
  table gives no escape wheel and no `--escape-teeth`, both common escape
  wheels are named, 15 teeth (30 beats a turn) and 20 teeth (40 beats,
  the ETA 2824 family and Rolex's): at 28,800 bph 3.75 s and 5 s, and the
  line the recording shows picks one. With no calibre, no wheel is named
  but the escape wheel `--escape-teeth` gives, if any: which wheel turns
  once a minute or once an hour is the calibre's to say. The JSON then
  gives 1 min and 1 h as `input.settings.reference_lines` (`label`,
  `period_s`), periods to mark on a chart, not wheels.
- `--wheel "third wheel=450"` (repeatable) names another wheel whose turn
  period you know.

`series` takes the same three. Both list the wheels they used in the
JSON's `input.settings.named_wheels`, and the reference lines in
`input.settings.reference_lines` (empty when a wheel is named).

## Calibrating the sound card

A sound card's crystal is typically 10–50 ppm off (1–4 s/d) and drifts
with temperature. With `--clock`, beat times are mapped onto true time
using a log, written during the recording, of how much audio had been
captured against the system clock (which NTP keeps right). One entry per
line, CSV or whitespace-separated, `#` comments, and an optional header:

```
time_ns,bytes
1791484354282000000,5760044
1791484414282100000,11520044
```

With a header, the audio column is `audio_s`, `frames`, `samples` or
`bytes` (of audio data), and the time column `unix_s`, `unix`, `time_s`
or `epoch` in seconds, or any name ending `_ns` or `_ms`. Without one,
the time column is the one that looks like a Unix time (seconds,
milliseconds or nanoseconds, told apart by size), and the audio column's
unit is whichever of seconds, frames or bytes makes it advance one second
per second. Only the slope matters, so a constant offset in the audio
count (a WAV header, a pipe buffer) does no harm. An entry every minute is plenty. Entries more than five
robust SDs off the fit (a timestamp taken late) are dropped. Runs over
four hours are mapped with a local straight-line fit every ten minutes
over two hours, which follows the crystal's drift with temperature.

Without a log, `long` uses the card's error stored with `timegrapher
clock` (see [agent-interface.md](agent-interface.md#clock)): the one for
`--device NAME`, else the one for the input the recording's sidecar or
`session.json` names. `--card-ppm N` gives the error directly. A stored
error is one steady number, so it cannot follow drift; the report says
which correction was used, and `summary.json`'s `input.settings.clock_correction`
records it.

## Takes with pauses

A take that was paused, or saved and then continued, has audio that runs
straight on across each break while the clock jumps. `long`, `series` and
`session` find the breaks in the take's `session.json` (`breaks`), plus
any step in `clock.csv` that none of those explains (for older takes,
the steps alone), and read each stretch on its own:
the clock fit takes the gap out before fitting, so a pause no longer reads
as a huge sound-card error; rate and beat error are fitted per stretch and
combined weighted by beats (where the beat error's sign turns over across
a break, as sizes, given as positive, and then the slices' beat errors
and `series`' are sizes too); no 10 s window, slice or `series` 2 s window reaches into a
break's gap from either side (a slice that would, or that has a 2 s
stretch with no beat in it, is left empty, rate, beat error and
amplitude alike, rather than read from part of its span), and the gap runs from the last sound before the pause to where
the clock log shows it took up again, so sound inside it belongs to
neither stretch; and the period search and Steadiness run on true time,
so a gap is a missing stretch. The JSON lists the breaks as `breaks`.
Where Tick and Tock were not told apart by their sound across a break,
the sides after it are named by the beat count across it; the printed
`Sides` line and the JSON's `caveats` (`sides_by_count`, `count`) say so.
Where the beat error is given as a size, a `Conditions` line ("beat error
given as a size: its sign differs either side of a cut") and the caveat
`beat_error_as_size` say so, as for `analyze`.
`analyze` on a single file does not look for breaks, so read a paused
take with `long`, `series` or `session`.

## How the search works

1. **Watch time.** Each series is placed on the watch's own clock: beat
   number times the nominal beat period. A wheel turns once per fixed
   number of beats (the fourth wheel every 480 at 28,800 bph), so on this
   clock its period is exact and its phase does not wander when the rate
   does.
2. **Series.** Timing offset from a steady rate (every clean beat, in
   0.25 s bins for runs up to 12 h, coarser beyond) and amplitude (2 s
   windows). Slow drift is removed with a running median of 30 minutes (a
   quadratic for runs under 20 minutes).
3. **Score.** The power spectrum is normalized using a local-median noise
   estimate. The score assumes exponential normalized noise power and a
   Gamma reference for sums of independent bins. Each trial period sums
   its first 1, 2, 4 or 8 harmonics, and the best sum is scored against that
   model, with an approximate allowance for searching many frequencies. A sinusoid
   scores best with one harmonic, a short event (a dip for four minutes
   every hour) with eight; twice the true period loses because half its
   harmonics are noise. Only harmonics inside the searched band count: a
   strong line just below the shortest period searched (a 5 s escape
   wheel under the 8 s floor of a run over 12 hours, whose timing is
   gridded every second) would otherwise lift every period it divides,
   10, 20 and 40 s. With default settings, a component must pass the
   model-based 1% whole-search threshold and explain at least 0.5% of the
   detrended variation. This is not a calibrated 1% false-positive guarantee
   for every noise process, missing-data pattern or repeated search.
4. **Fold and subtract.** The strongest period is refined, the series is
   folded at it (median per phase bin) to give the average cycle and the
   raster, that cycle is subtracted, and the search repeats. Wheel periods
   are integer ratios of each other, so without the subtraction one fault
   would appear at several periods.
5. **Name.** A component matches a wheel when their periods agree within
   the run's resolution (period squared over run length) or 1%. Otherwise
   the nearest wheel and how far off it is are given: a 57 s cycle in a
   two-hour run (resolution scale about 0.5 s) is not the fourth wheel.
   This scale, even when displayed with ±, is not a confidence interval for
   the fitted period; gaps, noise and the number of observed cycles matter.

The [precision and period-search assumptions](measurement-precision.md)
include reproducible model probes and their limits. A score is evidence of
repetition under the search model, not the probability that a particular
wheel is faulty. Slow drift and periodic variation can be real watch
behaviour; separating them for analysis does not make them measurement error.

Rate components are searched on the timing offset and reported as rate:
minus the slope of the average cycle, in s/d. A cycle shorter than 30 s
(three 10 s rate readings) is reported as the timing swing instead, in ms
peak to peak: an escape wheel's tooth pattern moves each beat by a fraction
of a millisecond, which as a slope is hundreds of s/d that no rate reading
would show. The JSON keeps `peak_to_peak` in s/d and adds
`timing_peak_to_peak_ms`, plus `size` and `size_unit` for the figure to
show.

The shortest period searched is eight bins of the series (the fold's
eight phase bins), so 2 s on the 0.25 s grid, 8 s on the 1 s grid of runs
over 12 hours, and 16 s for amplitude. With fewer bins per turn, a period
that is a whole number of bins folds onto a few fixed phases, and the slow
wander the trend leaves behind reads as a cycle. On a 16 h run, the
21,600 bph escape wheel's 5 s read 10.8 ms that way, where the beats show
0.5 ms.

## Limits

- Validated so far on synthetic recordings only
  (`crates/timegrapher-core/tests/long.rs`): no false cycles on a steady
  watch across chunk joins; a once-a-minute fault found at 60 s in rate
  and amplitude; an amplitude change alone not read as a rate change; a
  25 ppm sound-card error corrected. The 5-minute 3235 takes show no
  component above the 1% level: five turns of the fourth wheel are too
  few to tell a cycle from the watch's slower wandering.
- Periods longer than about half the 30-minute trend window are
  weakened, so the barrel (hours per turn) needs a separate slow analysis
  over days.
- One beat template, from the first chunk, is used for the whole run. A
  watch whose sound changes greatly over days (a weak mainspring at the
  end of the reserve) may lose beats late in the run; the clean
  percentage in the summary shows it.
