# Test sessions

`timegrapher session` reads one watch measured in several positions, and
perhaps at several states of wind, into one report: Witschi's
multi-position test (SEQ) and its characteristic values. Every recording
is analysed with the same engine as `analyze`, `shape` and `long`.

```sh
timegrapher session --init recordings/dandong-3235/   # writes session.toml
# edit the positions, wind and reference readings, then
timegrapher session recordings/dandong-3235/
```

The command prints a summary and writes `session_report/` next to the
session file: `report.html` (self-contained), `summary.json` (every number
in the report) and `readings.csv` (one row per recording).

Without a session file, `timegrapher session a.flac b.flac ...` (or a
folder) takes each recording's position from its file name and assumes
full wind.

## Session file

```toml
watch = "Submariner case, Dandong 3235 clone"
calibre = "3235"
bph = 28800
lift = 55             # 32xx: probably 55° (maybe 58°), per Peter
tolerance = "mens"     # ladies, mens, cosc-small, cosc, metas
settle_s = 20          # skipped at the start of each recording
# measure_s = 60       # measure only this long after settling
card_ppm = 19.3        # sound card's known error, for takes without a clock log

[[recording]]
file = "dandong_DU.flac"        # a file, or a folder of segments
position = "DU"                 # see below
wind_h = 0                      # hours since full wind
date = "2026-10-08T20:41Z"
reference = { rate = 1.0, amplitude = 290, beat_error = 0.3, source = "tg on the same file, lift 55" }   # example values

[[recording]]
file = "ym42_DU_2h"
position = "CH"
clock = "ym42_DU_2h/clocklog.txt"   # see long-runs.md
```

Paths are relative to the session file. Any recording-level `settle_s` or
`measure_s` overrides the session's. `shape = false` or `cycles = false`
at the top skips those parts. A `[limits]` table changes the project's
thresholds (below).

### Positions

| Witschi | Common | Pendant | Watch |
| --- | --- | --- | --- |
| CH | DU | | dial up |
| CB | DD | | dial down |
| 9H | CD | PD | crown down |
| 6H | CL | PL | crown left |
| 3H | CU | PU | crown up |
| 12H | CR | PR | crown right |

Names match in any case and also as words ("crown left"). In file names
the letter forms match in any case; 3H, 6H, 9H and 12H only in capitals,
so that `_2h` or `_12h` (a duration) is not taken for a position.

## What each reading is

After `settle_s` (default 20 s, Witschi's stabilisation time), to the end
of the recording or for `measure_s`:

- rate and beat error from one fit over every beat (tick and toc fitted
  together), corrected for the sound card's clock when the recording has a
  clock log or the session gives `card_ppm`;
- amplitude, the median of the 2 s windows;
- jitter, and the 5th to 95th percentile of rate in 10 s windows: how
  steady the rate is;
- periodic changes in rate and amplitude, from the same search as `long`,
  named after the wheel whose turn they match;
- beat shape (as `shape`) on the first 60 s after settling.

Repeat readings in the same position and state of wind are averaged.

## Characteristic values

Per state of wind (readings with the same `wind_h`), following Witschi:

| Value | Meaning |
| --- | --- |
| X, XH, XV | mean rate over all, horizontal and vertical positions |
| D, DV, DH | largest rate difference between positions; over vertical and horizontal positions only |
| DVH | vertical mean minus horizontal mean (rate and amplitude) |
| Di | 6H minus CH |

Across states of wind: isochronism per position (rate change from the
fullest to the least wound reading), Im (the largest, excluding 12H, as in
NIHS 93-10), Im\* (over all positions), Ie (change in X over the
positions measured in both) and N = 0.15·|Im| + 0.1·D + 0.6 (Witschi's
quality factor with its default thermal term).

## Findings

Witschi's tolerances, fully wound (readings within `full_wind_h`, default
2 h, of a full wind):

| Class | Rate s/d | Amplitude H | Amplitude V | Beat error |
| --- | --- | --- | --- | --- |
| Ladies' | −5…+25 | 260–320° | 240–280° | < 0.5 ms |
| Men's (default) | −5…+15 | same | same | same |
| COSC < 20 mm | −5…+8 | same | same | same |
| COSC > 20 mm | −4…+6 | same | same | same |
| METAS | 0…+5 | same | same | same |

Rules, each reported with the measurement that triggered it. Thresholds
marked *project* are this project's defaults where Witschi names the fault
without a number; change them under `[limits]`.

| Finding | Rule | Severity |
| --- | --- | --- |
| Overbanking | amplitude > 330° (Witschi; `overbanking`) | fault |
| Very low amplitude | < 200° (*project*, `amplitude_fault`) | fault |
| Large beat error | ≥ 2 ms (*project*, `beat_error_fault`) | fault |
| Amplitude outside tolerance | outside the class's H or V range, fully wound | check |
| Beat error outside tolerance | ≥ 0.5 ms | check |
| Mean rate outside tolerance | X outside the class's range, fully wound | check |
| Large differences between positions | D > 10 s/d (*project*, `delta_rate`) | check |
| Large vertical amplitude loss | DVH amplitude below −50° (*project*, `vh_amplitude_drop`) | check |
| Rate unsteady within a reading | 10 s rates spread > 20 s/d (*project*, `rate_spread`) | check |
| Regular change once per wheel turn | a periodic change above the 1% false-alarm level matching a wheel | check |
| Vertical and horizontal rates differ | \|DVH\| ≥ 5 s/d, with Witschi's pin advice | note |
| Periodic change tied to no listed wheel | as above, no wheel | note |
| Unlocking as loud as the drop; extra sounds around the beat | from the beat shape; not yet checked on watches with confirmed faults | note |
| Short measurement | < 40 s measured (Witschi's minimum; `min_measure_s`) | note |

Changes at whole fractions of a wheel's turn (20 s and 10 s beside a 60 s
cycle) come from a short, sharp change once per turn; they are listed with
that wheel's finding rather than on their own.

## For programs and agents

`summary.json` (and `--json`, which prints the same) carries every value
in the report, with `"schema": "timegrapher.session/1"`; the name changes
when a field changes meaning. Each finding has a stable `code`, its
`severity` (`fault`, `warning`, `note`), the `recording` it is about (an
index into `readings`, or null for findings across positions), and its
`evidence` and `advice` as text. Codes: `overbanking`,
`amplitude_very_low`, `amplitude_tolerance`, `beat_error_large`,
`beat_error_tolerance`, `rate_tolerance`, `positional_delta`,
`vh_amplitude_drop`, `rate_unsteady`, `cycle_wheel`, `cycle_other`,
`dvh_rate`, `shape_unlock_loud`, `shape_extra_sounds`,
`measurement_short`. Each reading's `verdicts` entry marks rate,
amplitude and beat error `within`, `outside` or `not_judged`.

## Validation

`crates/timegrapher-core/tests/session.rs` measures synthetic recordings
in six positions and two states of wind with known rates, amplitudes and
beat errors, and checks the readings, X, XH, XV, D, DV, DVH, Di, Im, Ie
and the findings that should and should not appear. Comparison with
Peter's bench timegrapher on the bench watches goes in
[validation.md](validation.md) as the recordings land.
