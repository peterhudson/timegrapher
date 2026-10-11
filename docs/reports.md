# Reporting a problem

`timegrapher report` makes one zip holding what a maintainer needs to
reproduce a problem. You can open the zip, check it and attach it to an
issue. Nothing is sent anywhere: you make the zip and attach it yourself.

```sh
timegrapher report take.flac --at 120 --span 300 --lift 52 --note "reads 310°, my No. 1900 says 265°"
timegrapher report --kind bug --note "the app froze when I unplugged the microphone"
timegrapher report --replay timegrapher-report-20261010T213559Z.zip
```

It never opens the microphone. A microphone check goes in only as the
recording and sidecar that `doctor --save` already wrote.

## What the zip holds

| File | What |
|---|---|
| `manifest.json` | A `timegrapher.report/1` document: the software (version and git hash), `created_utc`, `kind` (`bug`, `reading`, `setup`, `idea`), your `note`, `os_version`, the recording's name and the excerpt's place in it, the `commands` run (below), every file with its size and SHA-256, the `consent` flags, what was `redacted` and what was `left_out` |
| `audio/excerpt.flac` | `--span` seconds (default 300) from `--at` (default 0), or what there is. It holds the original samples, with the same rate, channels and sample size. It is a lossless cut, never a lossy re-encode. Five minutes at 48 kHz is about 18 MB, under GitHub's 25 MB limit for attachments. `--no-audio` leaves it out |
| `audio/excerpt.flac.json` | The recording's sidecar from `doctor --save` (device, configuration, mixer), if it has one, plus where the excerpt was cut from |
| `results/analyze-excerpt.json` | `analyze --json` on the excerpt |
| `results/analyze-whole.json` or `results/long-whole.json` | `analyze --json` on the whole file, or `long --json` on a folder of segments, so the size of any difference from the excerpt shows |
| `long/clocklog.txt` | A folder's clock log |
| `env/clock.json` | The stored sound-card clock corrections (`timegrapher clock`) |
| `logs/` | Log and crash files from the last 7 days in the settings folder |

`--bph` and `--lift` are passed to both analyses: give them as they were
set when the problem showed.

**Redaction.** Before anything is written, the home folder becomes `~`, and
whole-word occurrences of the user name and host name become `<user>` and
`<host>`. That covers the note, the results, the sidecar and the logs. The
analyses run beside the recording, so their paths are relative.
Environment variables are never included. `redacted` counts what was
removed, so a maintainer knows something was there.

**Consent.** `--listened-back` records that you played the excerpt and it
holds nothing private, such as voices. `--cc0` offers the excerpt to the
public test corpus under CC0 (see
[contributing-recordings.md](contributing-recordings.md)). Only the person
who made the recording should set either flag; an agent never sets them on
someone's behalf. Both are recorded only when the zip holds audio.

## `--replay`

This is the maintainer's half. `report --replay BUNDLE` takes the zip or its
unpacked folder and does three things:

1. It checks every file against the manifest's SHA-256 and lists any that
   changed or are missing.
2. It runs each recorded command whose input is in the bundle (the excerpt's
   `analyze`) with this build.
3. It compares what the command prints with the stored result. The
   `software` and `input` blocks are left out, because they describe the run
   rather than the reading.

Each command is reported as `Same`, `Moved` (with each value that changed,
then and now) or `Not run`. The whole-recording analysis is `Not run`,
because its input stays with the person. `--json` prints a
`timegrapher.replay/1` document with `bundle`, `damaged` and `commands`
(`args`, `output`, `ran`, `error`, `moved[]` of `path`, `then`, `now`).

A bad-reading report whose replay still shows the problem, and whose
recording has been run through tg, is a candidate take for the corpus.

## How the zip is made

The zip is written and read with the [`zip`](https://crates.io/crates/zip)
crate, and the manifest's checksums come from the
[`sha2`](https://crates.io/crates/sha2) crate (both in core `archive.rs`).
Entries are stored, not compressed, because the excerpt is FLAC already and
the rest is small; the same entries always make the same bytes. `--replay`
also reads deflated entries, so a bundle someone unpacked and zipped again
still replays. A damaged entry (a bad CRC or a cut-off file) is an error, and
a zip that would unpack to more than 1 GB is refused. Which paths a bundle
may hold and which commands it may run are checked by `report` itself, not
by the zip crate: no absolute paths, no `..`, and only the recorded
`analyze` command shape.

The excerpt is encoded with the [`flacenc`](https://crates.io/crates/flacenc)
crate (core `excerpt.rs`), in 4096-sample blocks, and keeps the recording's
samples exactly. Before a bundle is written, the excerpt is decoded again with
[`claxon`](https://crates.io/crates/claxon) and compared sample for sample
with what was cut. A difference is an error, so a bundle never carries audio
that is not the recording.
