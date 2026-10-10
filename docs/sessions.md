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

`escape_teeth` (default 15) sets the escape wheel's turn period, and
`wheels = { "escape wheel (20 teeth)" = 5.0, "third wheel" = 450.0 }`
names more wheels in the cycle search (seconds per turn).

Paths are relative to the session file. Any recording-level `settle_s` or
`measure_s` overrides the session's. `shape = false` or `cycles = false`
at the top skips those parts. A `[limits]` table changes the project's
thresholds (below).

One continuous take that moved through several positions is listed once
per position, each entry naming the same file or folder with `start_s`
and `end_s` (seconds into the recording; left out means its start or
end). Each stretch settles for `settle_s` from its own start, and its
reading is labelled with the stretch, e.g. `take (600–1200 s)`.

```toml
[[recording]]
file = "live_take"
position = "DU"
end_s = 600

[[recording]]
file = "live_take"
position = "CD"
start_s = 600
```

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

- rate from one fit over every beat (tick and toc fitted together),
  corrected for the sound card's clock when the recording has a clock log
  or the session gives `card_ppm`;
- beat error measured from the unlock, as tg and bench timegraphers do
  (median of the 2 s windows); this is the value judged against the
  tolerance. The beat error from the drop (the same fit as the rate) is
  shown beside it, and used when no unlock is found;
- amplitude, the median of the 2 s windows, with Tick and Tock beside it
  (the two kinds of beat, one per pallet stone; Tick is the even beats
  and Tock the odd ones, `amplitude_even_deg` and `amplitude_odd_deg` in
  the JSON, since the sound can't say which stone is which);
- jitter, and the median and the 5th to 95th percentile of rate in 10 s
  windows: how steady the rate is. The headline rate is the fit over
  every beat, the average the hands keep; where the rate wanders with
  dips (the Yacht-Master crown left), the 10 s median sits a few s/d
  above it, as tg's median reading sits above its mean, so compare tg's
  mean with the headline and tg's median with the 10 s median;
- periodic changes in rate and amplitude, from the same search as `long`,
  named after the wheel whose turn they match;
- beat shape (as `shape`) on the first 60 s after settling;
- whether amplitude and rate sit at two levels the watch switches
  between (see [Two states](#two-states)).

Repeat readings in the same position and state of wind are averaged.

## Two states

Some watches swing at two amplitudes and flip between them every few
tens of seconds, which a histogram of the readings shows as two humps.
`twostate.rs` looks for that in each reading, on amplitude (the 2 s
windows) and on rate (10 s readings), and the report gives one line per
reading, e.g. "two amplitude states, 239° and 250°, switching about
every 30 s (medium confidence)".

The windows are reduced to 10 s medians (outliers beyond 5 MAD dropped
first, so a stray window reading 130° is not a state) and a running
median over ±5 minutes is taken off, so the slow fall of amplitude as the
mainspring runs down is not read as two levels. One Gaussian and a
mixture of two of equal width are fitted to the blocks. Two states are
called only when:

| Test | Bar | Why |
|---|---|---|
| BIC, one level minus two | ≥ 10 | "very strong" evidence (Kass and Raftery) |
| Separation of the levels in widths (Ashman's D) | ≥ 2 | two humps, not one wide one |
| Share of the smaller state | ≥ 10% | |
| Switches | ≥ 4 | back and forth, not one step |
| Both states in each third of the reading | ≥ 5% of its blocks | not a drift |

Three outcomes other than two states:

- **regular**: the state sequence repeats at one lag with an
  autocorrelation of 0.5 or more, like the once-a-minute dip of the
  fourth wheel. That is a cycle (see the periodic changes), not a state.
- **measurement**: a change of the balance's swing moves Tick and Tock
  together. A split one side carries alone (the other side moves less
  than 40% as much, or the other way), or one where the beat error from
  the unlock jumps by 0.1 ms or more and over three times the change from
  the drop, is likely the unlock mark, not the watch.
- **one level**, with a note when the Tick (or Tock) 2 s windows on their
  own fall in two clusters that the other side's windows do not follow:
  the unlock mark hopping between the onset and the shoulder after sound
  1, as on the Daytona 4130, where 0.3 ms of edge is about 15°.

The edge check reads each 2 s window's unlock edge on each side (ms from
the beat time): when one side's edges fall in two clusters that the
other side's edges do not follow, the edge finder is hopping. A real
change of swing moves both sides' edges together (on the Yacht-Master
dial up, both by about 0.3 ms between its states).

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
| Unlock not timed reliably | under 60% of the 2 s amplitude windows find the unlock on both sides (*project*, `min_unlock_coverage`); amplitude and beat error from the unlock are then shown with `?` and not judged | check |
| Recording clipped | over 1% of the measured beats have a clipped sample in their template window, as `analyze` warns; amplitude and beat error may be off, rate is not | check |
| Rate unsteady within a reading | 10 s rates spread > 20 s/d (*project*, `rate_spread`) | check |
| Regular change once per wheel turn | a periodic change above the 1% false-alarm level matching a wheel | check |
| Vertical and horizontal rates differ | \|DVH\| ≥ 5 s/d, with Witschi's pin advice | note |
| Periodic change at a whole fraction of a wheel's turn | as above, the turn is a whole number (2 or more) of the period | note |
| Periodic change tied to no listed wheel | as above, no wheel | note |
| Unlocking as loud as the drop; extra sounds around the beat | from the beat shape; not yet checked on watches with confirmed faults | note |
| Short measurement | < 40 s measured (Witschi's minimum; `min_measure_s`) | note |

Changes at whole fractions of a wheel's turn (20 s and 10 s beside a 60 s
cycle) come from a short, sharp change once per turn; they are listed with
that wheel's finding rather than on their own. A rate change shorter than
30 s (an escape wheel's, for one) is too quick for the 10 s rate readings
to follow, so its size is the timing swing in ms, as `long` and `series`
state it; longer ones are in s/d. Each cycle in `summary.json` carries its
`size` with `size_unit` (`s/d`, `ms` or `deg`).

## For programs and agents

`summary.json` (and `--json`, which prints the same) carries every value
in the report, with `"schema": "timegrapher.session/1"`; the name changes
when a field changes meaning. Each finding has a stable `code`, its
`severity` (`fault`, `warning`, `note`), the `recording` it is about (an
index into `readings`, or null for findings across positions), and its
`evidence` and `advice` as text. Codes: `overbanking`,
`amplitude_very_low`, `amplitude_tolerance`, `beat_error_large`,
`beat_error_tolerance`, `rate_tolerance`, `positional_delta`,
`vh_amplitude_drop`, `rate_unsteady`, `cycle_wheel`, `cycle_wheel_fraction`, `cycle_other`,
`dvh_rate`, `shape_unlock_loud`, `shape_extra_sounds`,
`measurement_short`, `unlock_unreliable`. Each reading's `verdicts` entry marks rate,
amplitude and beat error `within`, `outside`, `not_judged` or
`unreliable`; `unlock_coverage` in its measurement is the share of
amplitude windows that found the unlock. `amplitude_states` and
`rate_states` in each measurement hold the two-state finder's result:
`verdict` (`one_level`, `two_states`, `regular`, `measurement`,
`too_short`), `confidence` (`low`, `medium`, `high`), the `low` and
`high` levels, `low_share`, `separation`, `delta_bic`, `switches`,
`dwell_low_s`, `dwell_high_s`, `switch_every_s`, `spread`, `period_s`
and `regularity`, and for amplitude `tick_change`, `tock_change`,
`beat_error_unlock_ms` and `beat_error_drop_ms` (low and high state),
`tick_unlock_ms` and `tock_unlock_ms` (low and high state),
`tick_split_alone`, `tock_split_alone`, `tick_edge_split_alone`,
`tock_edge_split_alone`, `one_sided` and `unlock_jump`.

## Validation

`crates/timegrapher-core/tests/session.rs` measures synthetic recordings
in six positions and two states of wind with known rates, amplitudes and
beat errors, and checks the readings, X, XH, XV, D, DV, DVH, Di, Im, Ie
and the findings that should and should not appear. Comparison with
Peter's bench timegrapher on the bench watches goes in
[validation.md](validation.md) as the recordings land.
