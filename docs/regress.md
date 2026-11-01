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

tg reads the same recording on the sound card's own clock, so a take whose
references are all on that clock (`clock = "sound_card"`, or no `clock`) is
read on it too: no clock log, USB lines or card correction is applied, and
rates are compared like with like. A take with a reference on its
instrument's own clock (`clock = "own"`) is read corrected, as `session`
reads it.

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
judged against tg alone.

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
  in its holder and bare, the Daytona replica about 2° below, the
  Yacht-Master replica up to 8° above crown down. The two methods place the
  unlock differently on some movements. At 4° about a third of holdout readings would fail for that
  known reason; at 8° a failure shows something new.
- A `[[recording]]` entry may name a fault confirmed at the bench:
  `fault = { id = "unlock_too_strong", side = "Tick", source = "bench: ..." }`
  (`side`, Tick, Tock or both, is optional). The program names no faults,
  so the entry is only carried into the revealed readings, beside the
  measured figures.
- It prints counts only: pass, fail and not judged for each value. No take
  names, file names or numbers, and `--fetch` does not name what it
  downloads.
- The run exits 0 whatever the counts say: a holdout result never blocks a
  change. Only a reading that cannot be measured fails it.
- `--reveal` also prints every reading's numbers, for the announced
  milestone run whose results go in each take's README. Run it locally,
  never in CI. `--json` prints the same as a `timegrapher.regress-holdout/2`
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
all (including a new reading without a baseline). A required baseline session
that is missing, or a trailing baseline reading removed from a session, fails
the run too. Takes deliberately excluded by `--take`, and holdout takes excluded
from a development run, are not required in that run.

Each existing recording's current label must match its baseline label exactly.
Explicit `start_s`/`end_s` intervals appear in labels rounded to whole seconds.
A changed label fails the run instead of accepting the recording as new.
Changes within the same rounded interval remain indistinguishable in this check;
it does not establish exact interval, audio-sample or settings identity.
The JSON reports these checks as `unreadable`, `missing_sessions`,
`missing_readings` and `recording_mismatches`, independently of numerical rows.

A reading also fails when its median amplitude is over 330° and so raises `session`'s
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
rewritten with it, provided it can be read. Appended readable recording entries
also remain new. `--write-baseline` accepts successful recording-label changes
(including changed intervals) and numerical movement after review. It refuses
to write when required sessions or readings are missing or unreadable, preserving
the existing baseline.
An absent baseline can be created; an existing baseline that cannot be read or
parsed is an error and is not replaced.

## In CI

The `regress (engine)` job in `.github/workflows/ci.yml` checks out the public
corpus at the full commit in its `CORPUS_REVISION` setting, currently
`692c7bcaa2c072d4075f4a85ce75564df962ab60`. It verifies the checkout's actual
commit and records that commit, the engine revision and `corpus.json`'s SHA-256
in the job log and summary before fetching audio. New corpus commits therefore
do not silently change an existing engine revision's CI inputs.

The job runs `regress corpus --fetch --smoke --baseline regress/baseline.json --quiet`. It needs no
secret, so pull requests from forks get the check too. The audio is cached
between runs and keyed on `corpus.json`, so a run downloads only what
changed. A required baseline take with no `session.toml` fails the check.
The `not_run` list also includes deliberately unselected takes; those do not fail.

### The app and MCP corpus checks

`regress (app)` and `regress (mcp)` read each corpus take as the app and the
MCP server read it, and compare with `timegrapher session` to the bit. On a
pull request they read a cut of the corpus made by `.github/ci/cut_corpus.py`:
each take's first three sound files and its last, with its `session.toml` and
`session.json` cut to match. Every push to main reads every file, and the CI
watchdog flags a failure on main on a "CI failed on main" issue. The engine
check reads the whole corpus on every run.

### Required real-recording equivalence

After regression, the same CI job reuses the fetched corpus to run every test
in `crates/timegrapher-core/tests/equivalence.rs` with the corpus required.
Two of them read real takes: `a_calibre_then_auto_at_the_found_rate_keeps_every_reading`
(the first two minutes of `dd3235_DU_30min`'s `take_part01-001.flac`, listened
to with a calibre picked and Auto again) and the
`analyze_and_session_agree_on_real_takes` test. The latter compares four full 600-second,
48 kHz files, each named `take_part01-001.flac`, from these takes:

- `daytona4130_DU_30min`
- `ym42_DD_30min`
- `patek324_DU_30min`
- `eta2824_holder_DU_30min`

This is the existing full-file `analyze` versus 47-second-chunk `session`
comparison: default analysis settings (including lift 52°), no clock correction,
and no settling interval. It checks rate, amplitude, signed unlock/drop beat
error and watch presence using the existing equivalence margins. It does not
compare against tg or the settled regression baseline. Live, replay and exact
beat-time equivalence remain separate; the four ignored legacy cases stay ignored,
`live_matches_analyze_on_real_takes` and `a_replayed_real_take_matches_session`
among them.

CI sets `TIMEGRAPHER_REQUIRE_CORPUS=1` and `TIMEGRAPHER_CORPUS` to the pinned
checkout. The test rejects an unset or invalid root, missing/ambiguous manifest
members, holdout takes, missing/empty/unreadable files, size/hash mismatches and
empty decoded audio. These known-watch fixtures must yield watch presence and
finite values for every compared quantity. Fixture paths, byte counts, SHA-256s
and the manifest digest are logged. The completed count is logged only after
all four comparisons pass. Output is retained in the job log and summary even
when the test fails; the CI step has a ten-minute limit.

To run that gate locally with these four files already fetched:

```sh
TIMEGRAPHER_CORPUS=/path/to/timegrapher-corpus TIMEGRAPHER_REQUIRE_CORPUS=1 \
  cargo test --locked --release -p timegrapher-core --test equivalence \
  analyze_and_session_agree_on_real_takes -- --exact --nocapture
```

Without either environment variable, local tests explicitly report a skip; that
test result supplies no acoustic validation. An explicitly supplied corpus must
contain all four authenticated inputs, even without the required-mode flag.
Metadata belongs in `corpus.json`; audio can be under `takes/<take>/` or directly
under `<take>/`. The fixture-validation unit cases use tiny constructed files
without acoustic analysis or process-wide environment changes.

## Updating CI's corpus revision

1. Compare the pinned corpus commit with the proposed full commit in a separate
   checkout. Review audio hashes and ordering, take folders, session/recording
   additions or removals, intervals, settings and reference sources/values. A
   metadata-only change can alter the comparison even when audio is unchanged.
2. In the engine pull request, set `CORPUS_REVISION` to the reviewed corpus commit
   and record the old/new commits, manifest hashes and reason for the update.
   Coordinate any required baseline session or reading removals in that same
   pull request: migrate or retire only reviewed entries, retaining their original
   values and explanation in version history. Missing sessions/readings now fail
   regression; `--write-baseline` cannot silently retire them. Do not narrow CI
   with `--take`. Never automatically regenerate baseline values or change
   numerical margins just to make CI pass.
3. Review intentional label/interval or numerical changes using the baseline
   procedure above. Preserve historical results and accepted offsets; corpus
   updates do not justify changing numerical margins. New readable recordings
   remain reported as new until their reviewed baseline entries are added.
4. Require the pull request's corpus regression to pass on its current engine
   head and confirm the logged corpus commit and manifest hash match the review.
   The cache remains keyed by the manifest: old cache entries are pruned against
   it, and `--fetch` checks each selected audio file's SHA-256 before reuse.
   A cache hit supplies audio bytes, not acceptance of new scientific results.

The holdout takes are read only by `.github/workflows/holdout.yml`, a job
started by hand from the Actions tab (`workflow_dispatch`) at a milestone.
It never runs on pull requests or pushes, and it prints counts only.

## Provenance of baseline entries

Future `--write-baseline` runs add `provenance` to each successfully replaced
reading. The baseline schema and numerical comparisons/margins are unchanged;
provenance is evidence, not a new input-mismatch acceptance gate. Existing
entries without it have **unknown origin**, serialized as `null` on a later
write. No engine version, comparator executable or historical hashes are inferred.
The top-level `software` identifies the document writer; a partial `--take`
update preserves the other entries' numbers and provenance, even when their
origins differ from that writer.

Each new entry records reported build metadata, the SHA-256 of the session
manifest bytes actually parsed, its relative path and one-based recording
index, the declared recording (including exact subsecond intervals/reference),
and the expanded decoder inputs in order, including duplicates. It records
observed file sizes, resolved stream settings, detected BPH, selected/measured
audio bounds, applied breaks and clock recipe. A clock CSV digest identifies
the bytes parsed. `clock_fit_reported_summary` is the existing public fit
summary: it does not serialize private clock interpolation knots. These data
are an inspection/reproduction aid, not a standalone replay archive.

`declared_corpus_identity` is what the corpus index declares. A separate
`verified_identity` is populated only from the existing `--fetch` SHA-256
verification pass, labelled `verified_at_fetch_this_run`, with its actual byte
count. Without that verification it is `null`, even if a declared digest exists.
Verification and decoding are separate operations; this does not establish an
atomic identity of the bytes decoded if files change during the run. No second
large-audio hash pass is added. File sizes are observed after analysis.

Per-entry `software` is the executable's reported build metadata, not a binary
digest or a guarantee that its build-time git/dirty fields are fresh. Comparator
numbers and `source` are copied literally from the manifest; comparator
executable, version and digest remain `null`. Provenance capture failure refuses
a baseline write. Ordinary comparison runs do not require provenance capture
and retain the existing numerical acceptance behavior. No tracked historical
baseline values are regenerated by this metadata change.
