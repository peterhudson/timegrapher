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
in the report) and `readings.csv` (one row per recording). The summary
ends with the report's full path. `report.html` is one page in English:
open it in any web browser, or attach it to an email to send it.

Without a session file, `timegrapher session a.flac b.flac ...` (or a
folder) takes each recording's position from its file name and assumes
full wind.

## Session file

```toml
watch = "Submariner case, Dandong 3235 clone"
calibre = "3235"
bph = 28800
lift = 55             # 32xx: probably 55° (maybe 58°), project value
settle_s = 20          # skipped at the start of each recording
# measure_s = 60       # measure only this long after settling
card_ppm = 19.3        # sound card's known error, for takes without a clock log

[[recording]]
file = "dandong_DU.flac"        # a file, or a folder of segments
position = "DU"                 # see below
wind_h = 0                      # hours since the watch was fully wound
date = "2026-10-08T20:41Z"
reference = { rate = 1.0, amplitude = 290, beat_error = 0.3, source = "tg on the same file, lift 55" }   # example values

[[recording]]
file = "ym42_DU_2h"
position = "CH"
clock = "ym42_DU_2h/clocklog.txt"   # see long-runs.md
```

A recording on a corpus holdout take may also carry
`fault = { id = "...", side = "Tick", source = "bench: ..." }`, the fault a
watchmaker confirmed; `session` reads past it, and `regress --holdout`
checks it (see [regress.md](regress.md#holdout-takes)).

`escape_teeth` (default 15) sets the escape wheel's turn period, and
`wheels = { "escape wheel (20 teeth)" = 5.0, "third wheel" = 450.0 }`
names more wheels in the cycle search (seconds per turn).

The `calibre` line, when the calibre table knows the name (the same table
the app's picker, `calibres` and `long --calibre` use; "3235", "2824-2" and
"4130 (clone)" all match), gives the beat rate, lift angle and escape wheel
where the session file leaves them out, and the train's wheels in the cycle
search. A name the table does not know is only a label. `--calibre NAME`
picks a calibre from the table in its place (an unknown name lists the
known ones). `--bph` and `--lift`, then the session file's `bph`, `lift`
and `escape_teeth`, win over the calibre's; where they disagree with it,
`session` says so (`note: the Dandong 4130's lift angle is 52°, the session
file says 55°; using 55°`), and `summary.json` lists the same notes under
`input.settings.calibre_notes`, beside the `calibre` used. With none of
them, the lift angle is the default 52°, and each reading carries the
`lift_unknown` caveat: its amplitude is graded one step lower, as
`analyze` grades it.

Paths are relative to the session file. Any recording-level `settle_s` or
`measure_s` overrides the session's. `shape = false` or `cycles = false`
at the top skips those parts. A `tolerance` line or a `[limits]` table,
which session files written before `timegrapher.session/2` carry, is read
and ignored: the report judges nothing against a tolerance.

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

Witschi's codes count round the dial with the crown at 3 o'clock, so crown
up is 3H and crown right 12H. The report names each position in words with
the code beside it, as the app does: "Crown Up (3H)", "Dial Down (CB)".

`wind_h` is the hours since the watch was fully wound: 0 just after
winding, 24 a day later. The report says it in words ("full wind", "24 h
after winding"). A reading without it shows "not given" and is taken as
fully wound; `session --init` leaves it commented out, so a take whose
winding is unknown is not logged as freshly wound.

## What each reading is

After `settle_s` (default 20 s, Witschi's stabilisation time), to the end
of the recording or for `measure_s`:

- rate from one fit over every beat (tick and toc fitted together),
  corrected for the sound card's clock when the recording has a clock log
  or the session gives `card_ppm`;
- beat error measured from the unlock, as tg and bench timegraphers do
  (median of the 2 s windows); this is the value the report shows first.
  The beat error from the drop (the same fit as the rate) is
  shown beside it, and used when no unlock is found. Both are shown as a
  size. Which beat is longer is not shown: Tick is the first beat the
  analysis locks on, so the sign can change from one run to the next.
  `summary.json` keeps the signed values;
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

Because the settling time is left out, and the sound card's clock is
corrected where it was logged, these readings can differ by a few s/d
from the quick check `record` prints while recording.

Repeat readings in the same position and state of wind are averaged.

A recording whose beats are no steadier than noise, as on an empty stand
(the median timing scatter is 2% of the beat period or more, the same
rule as `analyze`), shows no rate, amplitude, beat error, cycles or
shape: those numbers would describe noise. Its measurement has
`watch_heard` false, and its row and its finding read "No regular beat
found (timing scatter 8.6 ms, 2.4% of a beat)".

## Two states

Some watches swing at two amplitudes and flip between them every few
tens of seconds, which a histogram of the readings shows as two humps.
`twostate.rs` looks for that in each reading, on amplitude (the 2 s
windows) and on rate (10 s readings), and the report gives one line per
reading, e.g. "two amplitude levels, 239° and 250°, switching about every
30 s (ΔBIC 42, separation 3.1)".

The windows are reduced to 10 s medians (outliers beyond 5 MAD dropped
first, so a stray window reading 130° is not a level) and a running
median over ±5 minutes is taken off, so the slow fall of amplitude as the
mainspring runs down is not read as two levels. One Gaussian and a
mixture of two of equal width are fitted to the blocks. Two levels are
reported only when all of these hold (`two_levels` in the JSON):

| Test | Bar | Source |
|---|---|---|
| BIC, one level minus two (ΔBIC) | ≥ 10 | "very strong" evidence (Kass and Raftery) |
| Separation of the levels in widths (Ashman's D) | ≥ 2 | two humps, not one wide one (Ashman) |
| Share of the smaller level | ≥ 10% | project |
| Switches | ≥ 4 | project: back and forth, not one step |
| Both levels in each third of the reading | ≥ 5% of its blocks | project: not a drift |

Otherwise the line reads "one amplitude level". With fewer than 30
blocks it says how many there were and that they are too few.

Beside the levels, the line states what else the finder measured:

- "the low level comes back about every 60 s" when the level sequence
  repeats at one lag with an autocorrelation of 0.5 or more, as a cycle
  does (see the periodic changes);
- "Tick carries the change alone" (or Tock) when one side moves less than
  40% as much as the other, or only one side's 2 s windows or unlock
  edges fall in two clusters;
- "the beat error from the unlock jumps with it, the one from the drop
  does not" when the unlock beat error moves by 0.1 ms or more, and over
  three times as much as the drop's;
- with one level, "the Tick windows fall in two clusters" when one side's
  2 s windows split on their own.

What the levels mean for the watch, or for the measurement, is left to
the reader; the watchmaker skill discusses it.

## Characteristic values

Per state of wind (readings with the same `wind_h`), following Witschi:

| Value | Meaning |
| --- | --- |
| X, XH, XV | mean rate over all, horizontal and vertical positions |
| D, DV, DH | largest rate difference between positions; over vertical and horizontal positions only |
| DVH | vertical mean minus horizontal mean (rate and amplitude) |
| Di | 6H minus CH |

Across states of wind: isochronism per position (the change in rate,
amplitude and beat error from the fullest to the least wound reading;
`rate_change`, `amplitude_change` and `beat_error_change` in the JSON), Im (the largest, excluding 12H, as in
NIHS 93-10), Im\* (over all positions), Ie (change in X over the
positions measured in both).

## Findings

The findings are conditions found in the sound of one recording, each
with the figures that show it. None judges the watch: the report has no
tolerances, limits or advice. Published tolerances (Witschi's, COSC's,
the movement maker's) and what a figure means for the movement are in
the watchmaker skill, each with its source.

| Code | Title, e.g. | Listed when |
| --- | --- | --- |
| `no_regular_beat` | No regular beat found (timing scatter 8.6 ms, 2.4% of a beat) | the median timing scatter is 2% of the beat period or more |
| `unlock_unreliable` | Unlock found in 45% of windows | fewer than 60% of the 2 s amplitude windows find the unlock on both sides; amplitude and the beat error from the unlock rest on those windows |
| `clipping` | Clipped on 3.2% of beats | over 1% of the measured beats have a clipped sample in their template window, as `analyze` counts them |
| `measurement_short` | 25 s measured | under 40 s measured after settling |
| `cycle_wheel` | Regular rate change once per turn of the fourth wheel | a periodic change above the 1% false-alarm level matches a wheel's turn |
| `cycle_wheel_fraction` | Regular rate change at 1/3 of the fourth wheel's turn | as above, the turn is a whole number (2 or more) of the period |
| `cycle_other` | Regular amplitude change not tied to a listed wheel | as above, no wheel; the nearest is named |
| `shape_unlock_loud` | Unlocking as loud as the drop on Tick | the level of sound 1 to sound 3 is 1 or more |
| `shape_extra_sounds` | Extra sounds around the beat on Tock | sounds before the unlock or after the drop |

Every reading's table row also carries its unlock share and clipped
share, whether or not they are listed here.

Changes at whole fractions of a wheel's turn (20 s and 10 s beside a 60 s
cycle) come from a short, sharp change once per turn; they are listed with
that wheel's finding rather than on their own. A rate change shorter than
30 s (an escape wheel's, for one) is too quick for the 10 s rate readings
to follow, so its size is the timing swing in ms, as `long` and `series`
state it; longer ones are in s/d. Each cycle in `summary.json` carries its
`size` with `size_unit` (`s/d`, `ms` or `deg`).

## For programs and agents

`summary.json` (and `--json`, which prints the same) carries every value
in the report, with `"schema": "timegrapher.session/2"`; the name changes
when a field changes meaning. Each finding has a stable `code` (the table
above), the `recording` it is about (an index into `readings`), a `title`
and its `evidence` as text. `watch_heard` in a measurement is false when
no regular beat was found, and its readings are then null;
`unlock_coverage` is the share of amplitude windows that found the unlock,
`clipped_fraction` the share of clipped beats and `jitter_us` the median
timing scatter. `amplitude_states` and `rate_states` in each measurement
hold the two-state finder's figures: `two_levels`, the `low` and `high`
levels, `low_share`, `separation`, `delta_bic`, `switches`,
`dwell_low_s`, `dwell_high_s`, `switch_every_s`, `spread`, `period_s`
and `regularity`, and for amplitude `tick_change`, `tock_change`,
`beat_error_unlock_ms` and `beat_error_drop_ms` (low and high level),
`tick_unlock_ms` and `tock_unlock_ms` (low and high level),
`tick_split_alone`, `tock_split_alone`, `tick_edge_split_alone`,
`tock_edge_split_alone`, `one_sided` and `unlock_jump`.

Removed in `/2`: `report.tolerance`, `report.limits`, `report.verdicts`,
`report.n` (Witschi's quality factor), each finding's `severity` and
`advice`, the codes that judged against a tolerance or limit
(`amplitude_high`, `amplitude_very_low`, `amplitude_tolerance`,
`beat_error_large`, `beat_error_tolerance`, `rate_unsteady`,
`rate_tolerance`, `positional_delta`, `vh_amplitude_drop`, `dvh_rate`),
the two-state `verdict` and `confidence`, `input.settings.tolerance` and
the `--tolerance` flag. `no_watch_heard` is now `no_regular_beat`.

## Validation

`crates/timegrapher-core/tests/session.rs` measures synthetic recordings
in six positions and two states of wind with known rates, amplitudes and
beat errors, and checks the readings, X, XH, XV, D, DV, DVH, Di, Im, Ie
and that every finding is one of the codes above. Comparison with
The bench timegrapher on the bench watches goes in
[validation.md](validation.md) as the recordings land.
