# Regression check against tg

`timegrapher regress` measures every stored take again and compares the
engine with tg on the same file at the same lift angle. An engine change
that moves a reading away from tg fails the check, so it is caught before
the change is merged.

```sh
timegrapher regress ../timegrapher-recordings --baseline regress/baseline.json
```

The first argument is a folder of takes. Each take folder (up to three levels
down) holds a `session.toml` whose readings carry tg's numbers for that file
(`reference = { rate, amplitude, beat_error, source }`, see
[sessions](sessions.md)). The recordings themselves are private, in their
own repository; CI reads them with a token (below).

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
timegrapher regress ../timegrapher-recordings --baseline regress/baseline.json --write-baseline
```

`--take NAME` (repeatable) runs, or rewrites, only the takes whose folder
name contains NAME; the others stay as they were in the baseline. A new take
with no baseline entry shows as new and passes until the baseline is
rewritten with it.

## In CI

The `regress` job in `.github/workflows/ci.yml` checks out the recordings
repository with the `RECORDINGS_TOKEN` secret, a fine-grained personal
access token for that repository alone with read-only access to its
contents. A take too big for git keeps its audio in a release of that
repository named after the take's folder; the job downloads each such
release into its folder before the check. Without the secret, as on pull requests from forks, the job says
so and passes. It runs with `--quiet`, so the public log carries take names
and numbers only: no audio, recording file names or take notes.
