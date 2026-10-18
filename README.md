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
- **Tick-shape measurement.** Each beat has three sounds (unlock,
  impulse, drop). Their timing, levels and separation are measured on each
  pallet stone and reported as figures (see
  [docs/fault-signatures.md](docs/fault-signatures.md)).

The software is a measuring tool: it reports what it measured and the
conditions it detected (clipping, a short take, an unknown lift angle, no
regular beat found), with no grades, verdicts or fault names. Reading
those figures against a watch's tolerances is left to the person, or to an
AI assistant using the [watchmaker skill](skills/watchmaker/SKILL.md),
which cites a source for each judgement.

Status: early. The per-beat engine and a command-line tool analyse
recordings, from a few minutes (`analyze`, `shape`) to days (`long`), and
read a multi-position test (`session`). A
desktop app (egui) shows a watch live from a microphone, with the classic
paper-strip trace; see [docs/app.md](docs/app.md).

## Install

Download the desktop app and the command line tool for Linux, macOS or
Windows from the
[Releases page](https://github.com/peterhudson/timegrapher/releases/latest).
The builds are not signed, so macOS and Windows ask once per version before
opening them: [docs/install.md](docs/install.md) has the steps. To build
from source instead, use Rust 1.90.0 or later and see the
[package and platform requirements](docs/building.md).

## Desktop app

```sh
cargo run --release -p timegrapher-app            # live from a microphone
cargo run --release -p timegrapher-app -- rec.flac # analyse a recording
```

On Linux, building it needs the ALSA headers (`sudo apt install libasound2-dev`).

## Command line

```sh
cargo build --release --locked -p timegrapher-cli # Linux: libasound2-dev
./target/release/timegrapher doctor      # is the microphone set up right?
./target/release/timegrapher analyze recording.flac --lift 52
```

```
Duration     600.0 s at 48000 Hz
Beat rate    28800 bph, 4799 beats found (100.0% of those expected)
Rate         +8.9 ± 0.2 s/d, corrected for the sound card's clock (19.3 ppm slow, from 8 USB lines in the recording, 0.1 ppm apart); +10.5 s/d on the card's clock
Beat error   0.16 ± 0.01 ms (from the unlock; 0.26 ms from the drop)
Jitter       209 us per beat (within 10 s windows)
Rate spread  6.8 to 12.3 s/d (5th-95th percentile of 10 s windows)
Amplitude    298 ± 0.6 deg (Tick 298, Tock 297; lift angle 52 deg)
Shape        Tick: sound 1 in 98%, sound 2 in 100% of 299 windows; 1-2 2.35 ms, 1-3 7.10 ms, level 1/3 0.07, tail 0.12
             Tock: sound 1 in 100%, sound 2 in 100% of 299 windows; 1-2 2.24 ms, 1-3 7.06 ms, level 1/3 0.16, tail 0.17
             beat error from the unlock +0.17 ms in the first quarter, +0.14 ms in the last (75 windows each)
Periodic components in timing:
     154.8 s  explains   10% of variance
     100.1 s  explains    9% of variance
       5.0 s  explains    7% of variance
Periodic components in amplitude:
      46.4 s  explains   26% of variance
       5.0 s  explains   15% of variance
     107.6 s  explains    7% of variance
```

Options: `--bph` (guessed if omitted), `--lift` (degrees),
`--notch 5000,7000` (remove steady interference tones), `--escape-teeth`,
`--calibre NAME` (the movement's beat rate, lift angle and wheels from the
calibre table; `long`, `series`, `session` and `record` take it too, and
take its lift angle when `--lift` is not given),
`--beats beats.csv` (one row per beat), `--windows windows.csv` (rate,
beat error and amplitude over time, and where each side's unlock and drop
sat in each 2 s window, ms from the beat time), `--json`.

`timegrapher profile recording.flac --at 60 --svg profile.svg` shows the
sound of the Tick and the Tock over a 10 s stretch,
with the unlock, drop and sound marks behind the amplitude and beat error
readings: the place to look when those readings seem wrong.

`timegrapher long run.flac --clock clock.csv` analyses a long run in
bounded memory: rate and amplitude over time, the sound card calibrated
against NTP, and periodic changes found, named after the wheel they match,
and shown as an average cycle and a raster in an HTML report. See
[docs/long-runs.md](docs/long-runs.md).

`timegrapher devices` lists the sound inputs. `timegrapher doctor` records
a few seconds and checks level, clipping, automatic gain, noise and whether
ticks are heard; it names the exact mixer setting to change (on Linux, the
`amixer` command) and changes nothing unless given `--apply`.

### For AI agents

Every command takes `--json` and prints one document on a versioned schema
with the software version and the input it read. See
[docs/agent-interface.md](docs/agent-interface.md). The
[watchmaker skill](skills/watchmaker/SKILL.md) teaches an agent to run the
tool and interpret the figures it reports.

`timegrapher session folder/` reads one watch measured in several
positions (and states of wind) into a multi-position report in the style
of Witschi's SEQ mode: rate, amplitude and beat error per position, and
the characteristic values X, D, DV, DH, DVH, Di, Im and Ie, with the
measurements behind each. `--init` writes a
`session.toml` to edit. See [docs/sessions.md](docs/sessions.md).

`timegrapher synth out.wav` writes a synthetic recording with known rate,
beat error and amplitude, for testing; `--fault-period 60` adds a fault
that repeats like a bad tooth.

## How it works

1. **Envelope.** High-pass at 1.5 kHz (zero phase), optional notches,
   rectify, 0.2 ms moving average.
2. **Beat rate.** Autocorrelation scored at each standard rate.
3. **Beats.** A first pass follows the envelope's maxima, keeping each
   side (tick and tock) on one sound, and builds a median beat template
   with each side's beats moved onto that side's drop; the second pass
   correlates the whole recording with it and tracks one correlation peak
   per beat, and a template rebuilt from those beats is used for a final
   pass. Each beat gets a time and a quality score.
4. **Rate and beat error.** A least-squares fit of
   `t = t0 + k*P ± e/2` over all beats, robust to outliers, overall and in
   sliding windows. Beat times sit near the drop, so this is the drop's
   beat error.
5. **Amplitude and beat error from the unlock.** On median templates of
   2 s of beats, per side: unlock edge to drop edge, then
   `A = L / (2 sin(pi t / T))`. The same templates show how far each
   side's unlock sits from its beat times; moving the drop's beat error by
   the difference gives the beat error from the unlock, as tg and
   commercial timegraphers measure it. That is the headline figure (median
   over the windows), with the drop's alongside.
6. **Periodicity.** `analyze` bins timing residuals and amplitude to 1 s,
   detrends them and searches with a Lomb–Scargle periodogram. `long` and
   `series` use uniform grids and the FFT-based harmonic search described in
   [long runs](docs/long-runs.md#how-the-search-works). Periods near a wheel's
   turn can be labelled with it; the association is not a fault diagnosis.
   See the [model assumptions](docs/measurement-precision.md).

Validation against tg is in [docs/validation.md](docs/validation.md).

## Reporting problems and sharing recordings

Open an [issue](https://github.com/peterhudson/timegrapher/issues/new/choose):
there are forms for a wrong reading, a bug, an idea and for contributing a
recording of a watch to the public test corpus. A wrong reading can only be
fixed with the sound it came from; [docs/contributing-recordings.md](docs/contributing-recordings.md)
says what to record and how to cut it to size.

## Licence

GPL-3.0-or-later. This is a clean implementation; it contains no code from
tg (which is GPL-2.0-only), though it learned a great deal from it.
