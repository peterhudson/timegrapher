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
all. The beat error from the drop has no tg counterpart: when it moves by
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
