# Long runs

`timegrapher long` analyses a recording of hours or days: rate and
amplitude over the whole run, and any change that repeats with a wheel's
turn, which is how a damaged tooth, a worn pinion or an eccentric wheel
shows itself.

```sh
timegrapher long run.flac --clock run-clock.csv --lift 52
```

On a synthetic 30-minute recording whose watch loses 30 s/d and 15° of
amplitude for 8 s of every minute
(`timegrapher synth syn.wav --duration 1800 --rate 8 --snr 24 --fault-period 60`):

```
Recording    30:00 at 48000 Hz, 28800 bph, 14400 beats (100.0% clean)
Clock        not calibrated: absolute rate is only as good as the sound card (use --clock)
Rate         +4.00 s/d over the run; 10 s slices from -19.1 to 8.1 s/d (5th-95th pct)
Amplitude    268 deg median; slices from 253 to 268 deg (lift angle 52 deg)
Periodic changes in rate:
      60.00 s ±2.00   swing 36.8 s/d p-p     explains 100%  false alarm <1e-300  matches the fourth wheel
Periodic changes in amplitude:
      60.03 s ±2.00   swing 16.4 deg p-p     explains  42%  false alarm 4e-52    matches the fourth wheel
Report       syn_long/report.html
```

The rate swing reads high (36.8 against 30 s/d) because the average cycle
is kept to its first eight harmonics, which overshoot on a sharp step.

The recording is read in 47 s chunks, so memory use does not grow with
its length; a 30-minute file takes about 7 s on one core. Results go to a
folder next to the recording (`--out` to choose):

| File | Contents |
| --- | --- |
| `report.html` | The report: rate and amplitude over the run, the period searches, and for each component found its average cycle and a raster with one row per cycle. Self-contained; open it in a browser. |
| `summary.json` | The figures in the printed summary. |
| `beats.csv` | Every beat: number, time on the sound card's clock, calibrated time, match quality. Re-analysis needs only this and `amplitude.csv`. |
| `amplitude.csv` | Amplitude of even and odd beats, and beat error from the drop and from the unlock, in 2 s windows. |
| `slices.csv` | Rate, beat error (from the drop and from the unlock) and amplitude in slices of about 1/400 of the run. |
| `folds.csv` | The average cycle of each component found. |

A capture split into segments with no gaps is read as one recording by
listing the files in order (`timegrapher long seg01.flac seg02.flac ...`)
or by giving the folder that holds them, read in name order.

Options as for `analyze` (`--bph`, `--lift`, `--notch`, `--highpass`,
`--escape-teeth`), plus `--wheel "third wheel=450"` (repeatable) to name
other wheels whose turn period you know for the calibre.

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
3. **Score.** The power spectrum is divided by its local median, so under
   noise alone each frequency has a known distribution whatever the noise
   colour. Each trial period sums its first 1, 2, 4 or 8 harmonics, and
   the best sum converts to a chance of arising from noise. A sinusoid
   scores best with one harmonic, a short event (a dip for four minutes
   every hour) with eight; twice the true period loses because half its
   harmonics are noise. A component is reported when the chance of noise
   producing it anywhere in the search is under 1% and it explains at
   least 0.5% of the variation.
4. **Fold and subtract.** The strongest period is refined, the series is
   folded at it (median per phase bin) to give the average cycle and the
   raster, that cycle is subtracted, and the search repeats. Wheel periods
   are integer ratios of each other, so without the subtraction one fault
   would appear at several periods.
5. **Name.** A component matches a wheel when their periods agree within
   the run's resolution (period squared over run length) or 1%. Otherwise
   the nearest wheel and how far off it is are given: a 57 s cycle in a
   two-hour run (resolution ±0.5 s) is not the fourth wheel.

Rate components are searched on the timing offset and reported as rate:
minus the slope of the average cycle, in s/d.

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
