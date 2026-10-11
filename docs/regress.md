# Regression check against tg

`timegrapher regress` measures every stored take again and compares the
engine with tg on the same file at the same lift angle. An engine change
that moves a reading away from tg fails the check, so it is caught before
the change is merged.

```sh
git clone https://github.com/peterhudson/timegrapher-corpus
timegrapher regress timegrapher-corpus --fetch --smoke --baseline regress/baseline.json
```

The first argument is a folder of takes. Each take folder (up to three levels
down) holds a `session.toml` whose readings carry tg's numbers for that file
(`reference = { rate, amplitude, beat_error, source }`, see
[sessions](sessions.md)).

## The public corpus

The takes live in the public
[timegrapher-corpus](https://github.com/peterhudson/timegrapher-corpus)
repository, under CC0. Its git history holds each take's metadata under
`takes/<take>/`, and `corpus.json` lists every audio file with its size and
SHA-256. The audio itself is attached to a release named after the take.
When the first argument is a checkout of it (a folder with `corpus.json`),
`regress` reads the takes in its `takes/` folder.

- `--fetch` first downloads the audio of every take the run will read: those
  with a `session.toml`, and the smoke takes with `--smoke`, narrowed by
  `--take`. It skips files already present with the right SHA-256 and checks
  each download against `corpus.json`. It uses `curl`, which macOS, Windows
  10 and later and most Linux systems have.
- `--smoke` also runs the corpus's *smoke* takes, which have no tg reference,
  through the engine. Such a take fails if the engine panics. Stopping
  with an error is fine, and the beats found are listed so odd behaviour
  shows. The takes recorded with no watch on the stand
  (`silence_empty_stand`, listed in `corpus::NO_WATCH`) also fail unless
  `analyze` says no watch is heard (`watch_heard` false) in every file;
  their readings are not compared.

A folder of takes without `corpus.json`, such as a private set of
recordings, is read as it is.

## Holdout takes

The corpus's *holdout* tier (`"tier": "holdout"` in `corpus.json`; see "The
holdout seal" in the corpus README) holds recordings the engine is never
tuned on. Plain `regress`, `--fetch` and `--smoke` leave them out.
`regress corpus --holdout` reads only them, and no baseline: each reading is
judged against tg alone, and against the fault its session file names.

- Rate, amplitude and beat error (from the unlock, by size) pass within
  0.5 s/d, 8° and 0.2 ms of tg on the same file at the same lift angle
  (research's margins, `HOLDOUT_MARGINS`). A reading with no tg number for
  a value is counted as not judged.
- The amplitude margin is wide on purpose, and the count says it is against
  tg: tg is not the true amplitude. On the 90 referenced amplitude readings
  (`regress --all --json` on 814b681), the engine minus tg has a median size
  of 2.8°, 7.5° at the 90th percentile and 11.2° at the 95th; 93% are
  within 8°. The offset goes with the movement and its mount, not with the
  lift angle or the amplitude level: the ETA 2824 reads 7 to 21° above tg
  in its holder and bare, the Daytona about 2° below, the Yacht-Master up to
  8° above crown down. The two methods place the unlock differently on some
  movements. At 4° about a third of holdout readings would fail for that
  known reason; at 8° a failure shows something new.
- A `[[recording]]` entry may name a fault confirmed at the bench:
  `fault = { id = "unlock_too_strong", side = "Tick", source = "bench: ..." }`.
  `id` is one of `analyze`'s `faults[]` ids and `side` (Tick, Tock or both)
  is optional. The fault is found when `analyze`'s findings on the reading's
  measured stretch (after settling, at most 30 minutes of it) include that
  id on that side. An entry with no `fault` is a healthy watch, and any
  finding on it counts against the engine.
- It prints counts only: pass, fail and not judged for each value, and how
  many named faults were found or missed and how many findings were not the
  named fault. No take names, file names or numbers, and `--fetch` does not
  name what it downloads.
- The run exits 0 whatever the counts say: a holdout result never blocks a
  change. Only a reading that cannot be measured fails it.
- `--reveal` also prints every reading's numbers, for the announced
  milestone run whose results go in each take's README. Run it locally,
  never in CI. `--json` prints the same as a `timegrapher.regress-holdout/1`
  document.

## What fails

Each reading's rate (the fit over every beat), amplitude and beat error
from the unlock are compared with tg, and with the baseline: the same
readings measured by the engine the baseline was written with
([`regress/baseline.json`](../regress/baseline.json)). A value fails when it is
further from tg than the baseline was by more than its margin. Moving
closer to tg always passes, so does crossing tg at the same distance.

| Value | Margin | Compared with tg as |
|---|---|---|
| Rate | 0.3 s/d | the fit against tg's mean |
| Amplitude | 1.5° | the mean of Tick and Tock against tg's |
| Beat error from the unlock | 0.03 ms | its size against tg's (tg's is unsigned) |
| Beats found | 0.05%, at least 3 | the baseline: fewer beats fail |

A reading also fails when it loses a value it had, or cannot be read at
all, or when its median amplitude is over 330° and so raises `session`'s
amplitude check (docs/sessions.md): no take in the corpus does, and one
that starts to is a sign the engine reads amplitude high. The JSON lists
such readings in `amplitude_high`, as `take #reading`. The beat error from the drop has no tg counterpart: when it moves by
more than 0.03 ms it is listed as changed, without failing.

The margins sit just above how far readings move between engine builds that
should not change them: on the takes measured so far, rate moved under
0.02 s/d, amplitude about 0.2° and beat error 0.005 ms. Change them with
`--rate-margin`, `--amplitude-margin` and `--beat-error-margin`.

The text output starts with the engine's distance from tg on each take
(the worst reading of each), then lists every value that moved, then the
counts and `PASS` or `FAIL`. Exit code `3` is a failure; `--json` prints
the same as a `timegrapher.regress/1` document.

## When an engine change is meant to move the numbers

Run the check, read what moved, and when every move is wanted write a new
baseline in the same pull request:

```sh
timegrapher regress timegrapher-corpus --fetch --baseline regress/baseline.json --write-baseline
```

`--take NAME` (repeatable) runs, or rewrites, only the takes whose folder
name contains NAME; the others stay as they were in the baseline. A new take
with no baseline entry shows as new and passes until the baseline is
rewritten with it.

## In CI

The `regress` job in `.github/workflows/ci.yml` checks out the public
corpus and runs `regress corpus --fetch --smoke --quiet`. It needs no
secret, so pull requests from forks get the check too. The audio is cached
between runs and keyed on `corpus.json`, so a run downloads only what
changed. A take in the baseline that the corpus has no `session.toml` for
yet is listed as not run, which does not fail the check.

The holdout takes are read only by `.github/workflows/holdout.yml`, a job
started by hand from the Actions tab (`workflow_dispatch`) at a milestone.
It never runs on pull requests or pushes, and it prints counts only.
