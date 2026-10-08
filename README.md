# timegrapher

An open-source timegrapher for mechanical watches that measures **every
beat** and keeps the record. Most timegraphers, including the open-source
[tg](https://github.com/vacaboja/tg) family, average a few seconds of sound
into each reading. Measuring each beat makes two kinds of analysis
possible that no open-source tool offers today:

- **Gear-train analysis over hours or days.** A damaged tooth or an
  eccentric wheel repeats once per turn of its wheel. Searching the rate
  and amplitude for periodic components points at the wheel: the escape
  wheel turns every few seconds, the fourth wheel every minute, the centre
  wheel every hour.
- **Tick-shape characterisation.** Each beat has three sounds (unlock,
  impulse, drop). Their timing and shape on each pallet stone reveal
  escapement faults (see [docs/fault-signatures.md](docs/fault-signatures.md)).

Status: early. This first milestone is the per-beat engine and a
command-line tool that analyses recordings. A live desktop app (egui)
comes next.

## Command line

```sh
cargo build --release
./target/release/timegrapher analyze recording.flac --lift 52
```

```
Duration     300.0 s at 48000 Hz
Beat rate    28800 bph, 2400 beats found
Rate         +24.5 s/d (uncalibrated sound-card clock)
Beat error   0.08 ms
Jitter       166 us per beat (within 10 s windows)
Rate spread  -0.9 to 49.0 s/d (5th-95th percentile of 10 s windows)
Amplitude    232 deg (even beats 233, odd beats 232; lift angle 52 deg)
Periodic components in timing:
      59.3 s  explains   11% of variance  (fourth wheel?)
Periodic components in amplitude:
      56.7 s  explains   38% of variance  (fourth wheel?)
```

Options: `--bph` (guessed if omitted), `--lift` (degrees),
`--notch 5000,7000` (remove steady interference tones), `--escape-teeth`,
`--beats beats.csv` (one row per beat), `--windows windows.csv` (rate and
amplitude over time), `--json`.

`timegrapher synth out.wav` writes a synthetic recording with known rate,
beat error and amplitude, for testing.

## How it works

1. **Envelope.** High-pass at 1.5 kHz (zero phase), optional notches,
   rectify, 0.2 ms moving average.
2. **Beat rate.** Autocorrelation scored at each standard rate.
3. **Beats.** A first pass builds a median beat template; the second pass
   correlates the whole recording with it and tracks one correlation peak
   per beat. Each beat gets a time and a quality score.
4. **Rate and beat error.** A least-squares fit of
   `t = t0 + k*P ± e/2` over all beats, robust to outliers, overall and in
   sliding windows.
5. **Amplitude.** On median templates of 2 s of beats, per side: unlock
   edge to drop edge, then `A = L / (2 sin(pi t / T))`.
6. **Periodicity.** Timing residuals and amplitude binned to 1 s, detrended,
   and searched with a Lomb–Scargle periodogram; peaks near a wheel's turn
   are labelled with it.

Validation against tg is in [docs/validation.md](docs/validation.md).

## Licence

GPL-3.0-or-later. This is a clean implementation; it contains no code from
tg (which is GPL-2.0-only), though it learned a great deal from it.
