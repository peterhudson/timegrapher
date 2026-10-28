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
reference = { rate = 1.0, amplitude = 290, beat_error = 0.3, source = "tg on the same file, lift 55", clock = "sound_card" }   # example values

[[recording]]
file = "ym42_DU_2h"
position = "CH"
clock = "ym42_DU_2h/clocklog.txt"   # see long-runs.md
```

When a session manifest is consumed, `bph` must be a positive `u32`,
`lift` (including its `lift_angle` alias) must be finite and positive,
`escape_teeth` must be a positive `u32`, and `card_ppm` must be finite with
`1 + card_ppm × 10⁻⁶ > 0`. These are representation/mathematical domains,
not watch operating ranges. Zero clock correction remains valid. Missing
fields, labels, lift context and valid-setting precedence are unchanged.

CLI session/record reject an invalid stored field before audio or take/report
writes, even if a command-line flag would override it. Regression manifest
reads use the same checks. The desktop rejects invalid settings in a manifest
that lists the opened recording, before starting replay or analysis; a valid
TOML manifest that does not list that recording remains unrelated and is not
validated for these fields. Applicability retains the existing exact-path
matching: child-file entries alone do not attach settings to the folder view;
a folder entry such as `file = "."` can apply to that folder. A syntactically
corrupt neighboring manifest cannot establish applicability and now fails
opening instead of falling back to defaults. No-manifest opening keeps its previous behavior.

This check is explicit at those consumption points. It does not change
Serde parsing/serialization, validate programmatically constructed settings,
or validate other numeric fields, custom catalogue entries or clock stores.
Invalid persisted data must be repaired explicitly; it is not normalized or
rewritten by a failed read.

`record` and `session` use the custom names, aliases and wheel overrides
shown by `calibres`, using one resolved catalogue snapshot per invocation.
Session flags win over manifest values/context; catalogue values fill only
missing fields, then existing defaults apply. A custom name without a lift
value does not make the default lift known. An explicit unknown `--calibre`
fails; an unknown manifest name remains a descriptive label. Named selections
report malformed catalogue errors before reading audio.

A new `record` session stores effective numeric BPH/lift and lift context,
using the catalogue's canonical name. Appending preserves the parent manifest
text and label, including an alias; each take records its effective settings.
Legacy calibre-only manifests inherit missing values from the current
catalogue at each invocation. Their past settings cannot be recovered from
the label alone, and appending does not silently rewrite that history.
The session JSON's `input.settings.calibre` identifies the resolved canonical
selection; its descriptive `calibre` field can retain the original label.
If `record --calibre` overrides an unknown or different parent label, a later
`session` may need the same explicit override to interpret fields the parent
did not store. Appending still preserves that parent label and enforces its
existing numeric/context consistency checks.

New desktop and `record` session files pair `lift` with `lift_known = true`
(supplied or taken from an actual calibre lift value) or `false` (assumed).
This records the setting's context, not a verified physical lift angle. The
marker applies only to its accompanying numeric `lift`; an explicit CLI
`--lift`, including 52, wins over both. A calibre name without a lift value
does not make an assumed angle known.

Older files have no recoverable provenance marker. The CLI retains its legacy
interpretation that a numeric manifest lift is supplied; the desktop retains
its default-angle inference, using actual calibre lift data rather than a name
alone. The desktop preserves newly restored context when the value is edited
and reverted. Entering or reselecting an unchanged 52 in its existing controls
still cannot distinguish a new explicit confirmation from the default.
Older program versions that reject unknown manifest fields cannot read the new
marker; removing it restores their legacy inference, not the original context.

When appending a `record` take, an explicit lift cannot silently replace an
existing session's assumed context, even at the same numeric value. Use another
folder, or edit `lift_known = true` only if that supplied context applies to all
of the session's takes. Existing context is retained when no new lift is given.

A `reference` can describe tg on the same file or a bench instrument. Its
`source` is descriptive text. Its `clock` says which clock its rate was
read on:

- `clock = "own"`: the reference instrument's own time base, such as a
  bench timegrapher's crystal listening to the watch beside this
  recording. Its rate is set beside a clock-corrected reading (a clock
  log, or a stored or given correction).
- `clock = "sound_card"`: this sound card's clock, as for tg or another
  program reading the same recording. Its rate is set beside an
  uncorrected reading.
- Left out: not recorded. Its rate is set beside an uncorrected reading,
  as before; check the source for a matching basis.

The HTML report labels the measured rate as sound-card-clock or clock
corrected and the reference by its clock, and keeps every supplied value.
The rate Difference is given only where the two clocks match, and is "—"
otherwise (as for a clock-corrected reading, an explicit zero-ppm
correction included, beside a reference whose clock is not recorded). A
reference's own clock error is not corrected. The rate chart overlays the
reference rates only when every reading is on one clock and every
reference rate on that clock too, and otherwise says why it leaves them
out; the table keeps all reference values. Amplitude comparisons and the
calibrated headline rate are unchanged. The JSON gives each reading's
`reference` as the session file set it, `clock` included when given.

A recording on a corpus holdout take may also carry
`fault = { id = "...", side = "Tick", source = "bench: ..." }`, the fault a
watchmaker confirmed; `session` reads past it, and `regress --holdout`
checks it (see [regress.md](regress.md#holdout-takes)).

`escape_teeth` (default 15 when the calibre gives none) sets the escape
wheel's turn period, and
`wheels = { "escape wheel (20 teeth)" = 5.0, "third wheel" = 450.0 }`
names more wheels in the cycle search (seconds per turn). With no
calibre, only the wheels these two give are named.

The `calibre` line, when the calibre table knows the name (the same table
the app's picker, `calibres` and `long --calibre` use; "3235", "2824-2" and
"DD4130" all match), gives the beat rate, lift angle and escape wheel
where the session file leaves them out, and the train's wheels in the cycle
search. A name the table does not know is only a label. `--calibre NAME`
picks a calibre from the table in its place (an unknown name lists the
known ones). `--bph` and `--lift`, then the session file's `bph`, `lift`
and `escape_teeth`, win over the calibre's; where they disagree with it,
`session` says so (`note: the Dandong 4130's lift angle is 52°, the session
file says 55°; using 55°`), and `summary.json` lists the same notes under
`input.settings.calibre_notes`, beside the `calibre` used. With none of
them, the lift angle is the default 52°, and each reading carries the
`lift_unknown` caveat.

Paths are relative to the session file. Any recording-level `settle_s` or
`measure_s` overrides the session's. `shape = false` or `cycles = false`
at the top skips those parts. A `tolerance` line or a `[limits]` table,
which session files written before `timegrapher.session/2` carry, is read
and ignored: the report judges nothing against a tolerance.

A recording's `masked` list leaves stretches out of its reading: each an
`[start, end]` pair in seconds into the file or folder (the sound's own
seconds, which leave out any pause), such as `masked = [[600.0, 612.5]]`
for a bump of the stand. The app writes it when stretches were masked on
its Paper Strip. A masked stretch is cut at both ends, as the gap of a
restart in listening is: its beats and the amplitude windows reaching
into it are left out, and no fit, 10 s window or two-state sample reaches
into it from either side, so the few beats at its edge that fill no
whole 10 s window are left out of the 10 s rates rather than averaged
across it. The beats expected and the time listened to leave out its
seconds, and the reading's `measurement` gives `masked_s` (the masked
seconds inside the measured stretch) and the caveat `masked`
(`seconds`). The seconds measured (`measured_s` in `readings.csv`, the
report's Measured column and the `measurement_short` finding) leave out
the masked seconds, and the beat shape is measured on unmasked sound: the
first unmasked minute after settling, else the longest unmasked stretch;
when everything after settling is masked there is no shape. The app
lists every masked stretch on every recording; each counts only the part
inside its own stretch. Overlapping pairs are joined; a pair whose end
is not after its start is ignored. Builds from before `masked` refuse a
session file that has it, as they refuse any field they don't know.

Where the app's Position Brush painted over the positions recorded, the
session file also lists what was recorded, so the app can bring it back:
top-level `[[recorded]]` entries, one per stretch, each with `position`
and, as a recording's `start_s` and `end_s`, the stretch's start once
settled and its end, in the sound's own seconds. Readings take the
positions from the `[[recording]]` entries alone; `timegrapher session`
reads `[[recorded]]` and ignores it. Builds from before `recorded`
refuse a session file that has it, as they refuse any field they don't
know.

One continuous take that moved through several positions is listed once
per position, each entry naming the same file or folder with `start_s`
and `end_s` (seconds into the recording; left out means its start or
end). Each stretch settles for `settle_s` from its own start, and its
reading is labelled with the stretch, e.g. `take (600–1200 s)`. An
entry whose `start_s` equals its `end_s` is a position that settled
right to its end (the app saves one so when the watch was moved again,
or painted over, before it settled): it has nothing to read.
`timegrapher session` and `regress` leave it out with a note, the Watches list reads
the last entry that has something to read, and a session file with
nothing else to read is refused.

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
up is 3H and crown right 12H. The report's tables and the text table
name each position as the app does: in words with the common code, and
Witschi's beside it for the crown positions: "Crown Up (CU, 3H)", "Dial
Down (DD)". A finding's `evidence`, shown as it is in the JSON and in
the findings of the summary and the report, keeps Witschi's code alone:
"Crown Up (3H)", "Dial Down (CB)"; the charts, the facts line and the
Im line ("excluding 12H") show Witschi's codes.

`wind_h` is the hours since the watch was fully wound: 0 just after
winding, 24 a day later. The report says it in words ("full wind", "24 h
after winding"). A reading without it shows "not given" and is taken as
fully wound; `session --init` leaves it commented out, so a take whose
winding is unknown is not logged as freshly wound.

## A take kept as its analysis alone

The app's **Save Analysis Only…**, and **Keep Analysis Only…** on a
saved take in the Watches pane, keep a take without its sound: a take
folder in the standard layout with every audio file left out and an
`analysis/` folder in their place. `session.toml` says `audio = false` at
the top; its recordings, positions and masks are as for the take with its
sound, every entry naming the whole take (`file = "."`). `session.json`
and `clock.csv` are unchanged (`session.json`'s `segments` still name the
files the sound was in), and `positions-test.json` is kept when there is
one. With no audio files, the file-order rule has nothing to order. A
folder with both sound and `analysis/`, with `analysis/` but no
`audio = false`, or with `audio = false` but no `analysis/`, is refused
with a message saying so; builds from before
`audio` refuse the session file, as they refuse any field they don't
know.

`timegrapher session` reads such a take's beats from `analysis/` instead
of finding them in the sound, and everything after that is read as for
the sound: the breaks from `session.json` (or the clock log), the masks,
the readings, the two states and the cycles. A lift angle other than the
one kept (`--lift`, `--calibre`, or one edited into `session.toml`)
rescales the kept amplitudes to it, as the sound would read at that
lift. Each position's beat shape is the one measured on the sound when
the take was kept (the first unmasked minute after the session file's
settling, as above, whatever `--settle` says now, and whatever masks
were changed since). `input.settings.analysis_only` gives the fact
(`note`) and `measured_by` (the software that measured the sound: name,
version and commit); it is `null` for a take with its sound. The report
adds a **Sound** row saying none was kept and what measured it.

| File in `analysis/` | Contents |
| --- | --- |
| `analysis.json` | The header, below |
| `beats.csv.gz` | Every beat, gzipped CSV with the header line `index_step,residual_us,quality,clipped`: the step in the beat's number from the beat before (from 0 for the first; the numbers are the beat log's own, after Tick and Tock are matched across jumps, so their parity gives the side), the beat's time in whole µs less where the beat before and that many nominal beats at the header's `bph` put it (from 0 µs for the first), its quality written in full (the shortest decimal that reads back to the same value), and 1 when it clipped, else 0. Times read back to within 0.5 µs |
| `windows.csv.gz` | Every 2-second amplitude window, gzipped CSV with the header line `start_s,end_s,even_deg,odd_deg,beat_error_ms,beat_error_unlock_ms,even_unlock_ms,odd_unlock_ms,even_drop_ms,odd_drop_ms` (the beat log's fields, each written so it reads back exactly; empty where there is none) |
| `profiles.bin.gz` | The Tick Tock profiles of each 2 seconds, gzipped binary, below |

`analysis.json` holds `format` (`"timegrapher.analysis/1"`; a later
version is refused by name), `software` (name, version and commit of
what measured the sound), `audio` (false), `note` (the fact: "Saved
without its sound: it can't be measured again, and the waveform (the
Scope pane and Replay) can't come back."), `sample_rate`,
`bytes_per_frame` (of the sound, for reading `clock.csv`), `duration_s`,
`bph`, `lift_deg`, `clipped_samples`, `jumps` (audio seconds where the
beats jumped), `sided` (the jumps whose sides the sound told apart; the
others are named by the beat count alone), `settings` (the analysis settings that shaped the beats:
`bph` as given, `lift_deg`, `lift_known`, `onset_fraction`,
`escape_teeth`, `rate_window_s`, `amplitude_window_s`, the `envelope`'s
`highpass_hz`, `notch_hz`, `smooth_s` and `burst_gate`, the stream's
`chunk_s` and `margin_s`, and `profile_every_s`), the row counts `beats`,
`windows` and `profiles`, `usb_clock` (`ppm`, `lines`, `spread_ppm`: the
card's clock error from the USB lines in the sound's first ten minutes,
or null), `signal_median` (the median signal figure, each beat's peak
over the envelope's median, or null) and `shape` (one entry per
recording with a shape: `recording`, its place in `session.toml` from
0, `from_s` and `to_s`, the stretch measured, and `summary`, as a
reading's `shape`).

`profiles.bin.gz` is little-endian. It starts with the 8 bytes
`TGPROF\0\x01`, the stretch length (f32 seconds, 2), the first point
and the spacing between points (f32 ms each: −20, and every second point
of the Tick Tock Profile pane's, about 0.2), the points in a curve
(u16), two bytes of 0 and the number of stretches (u32). Each stretch
then has its end (f64 seconds; it began 2 seconds before) and its signal
figure (f32, NaN when none), then Tick and Tock in turn. Each is one byte,
0 when there is no profile, or 1 followed by the beats in it (u16), the
top and the floor (f32 envelope levels), the unlock, drop, peak,
amplitude and sounds 1 to 3 (f32 ms, or degrees for the amplitude; NaN
when none), then the median, 10th and 90th percentile curves, a byte a
point: the level in quarter-dB steps above 63.75 dB below the top (0 for
anything lower, read back as 0). Stretches with no beats are left out.

## What each reading is

After `settle_s` (default 20 s: the "20 s stabilisation time" of Witschi's
*Testing Methods for Mechanical Watches*, D40e, 2025, measurement tips, about p. 34), to the end
of the recording or for `measure_s`:

- rate from one fit over every beat (Tick and Tock fitted together),
  corrected for the sound card's clock: by the clock log the session file
  names, with the recording's USB lines setting its rate when it has them;
  without a log, by `card_ppm` or a correction stored for the input, else by
  the USB lines, as `series` and `long` correct it. A reading with a mask or a restart in
  listening is cut at both ends of each: each stretch between cuts is
  fitted on its own, a stretch whose beats span under 10 s is left out,
  and the rest are combined weighted by their beats;
- beat error measured from the unlock, as tg and bench timegraphers do
  (median of the 2 s windows); this is the value the report shows first.
  The beat error from the drop (the same fit as the rate) is
  shown beside it, and used when no unlock is found. Both are shown as a
  size. Which beat is longer is not shown: Tick is the first beat the
  analysis locks on, so the sign can change from one run to the next.
  `summary.json` keeps the signed values;
- amplitude, the median of the 2 s windows, with Tick and Tock beside it
  (the two kinds of beat, one per pallet stone, since the sound can't
  say which stone is which; in the JSON, Tick is `amplitude_even_deg`
  and Tock `amplitude_odd_deg`);
- jitter, and the median and the 5th to 95th percentile of rate in 10 s
  windows: how steady the rate is. A 10 s window is used only when each
  2 s of it holds a beat and it reaches into no cut; it needs no beat at
  or after its end, so a stretch of exactly 10 s has one. The headline rate is the fit over
  every beat (over the stretches between cuts, as above), the average the
  hands keep; where the rate wanders with dips (the Yacht-Master crown
  left), the 10 s median sits a few s/d above it, as tg's median reading
  sits above its mean, so compare tg's mean with the headline and tg's
  median with the 10 s median;
- periodic changes in rate and amplitude, from the same search as `long`,
  named after the wheel whose turn they match;
- beat shape (as `shape`) on the first 60 s after settling, or on
  unmasked sound when the recording has `masked` (see above);
- whether amplitude and rate sit at two levels the watch switches
  between (see [Two states](#two-states)).

The quick check `record` prints while recording skips the `settle_s` of
the session folder's `session.toml` (30 s when the file has none) and
reads the rest on the sound card's clock, so it agrees with this report
on a session file `record` creates (which sets `settle_s = 30.0` and
names no clock log). It can differ where a reading uses what the check
does not: `--settle`, `--bph`, `--lift` or `--calibre`, a reading's own
`settle_s`, `measure_s`, `start_s` or `end_s`, a clock log or
`card_ppm`, the 20 s default on a file with no `settle_s`, or a
restart in listening within the take, where this report fits the reading
in pieces between the restarts (still one row) and the check fits it
whole.

Repeat readings in the same position and state of wind are averaged.

A recording whose beats are no steadier than noise, as on an empty stand
(the median timing scatter is 2% of the beat period or more, the same
rule as `analyze`), shows no rate, amplitude, beat error, cycles or
shape: those numbers would describe noise. Its measurement has
`watch_heard` false, and its row and its finding read "No regular beat
found (timing scatter 8.6 ms, 6.9% of a beat)" for a 28,800 bph watch.

## Two states

Some watches swing at two amplitudes and flip between them every few
tens of seconds, which a histogram of the readings shows as two humps.
`twostate.rs` looks for that in each reading, on amplitude (the 2 s
windows) and on rate (10 s readings), and the report gives one line per
reading, e.g. "two amplitude levels, 239° and 250°, switching about every
30 s (ΔBIC 42, separation 3.1)".

The windows are reduced to 10 s medians (outliers beyond 5 MAD dropped
first, so a stray window reading 130° is not a level; but when those on
one side are at least 10% of the windows they are kept, since a level
held under half the time can sit that far from the median) and a running
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

A block is missing where the reading was masked or paused, or where too
few windows filled it. The finder keeps each block's place in the
reading, so a hole is time that passed: the running median takes the
blocks within ±5 minutes of the take's time, only a change between two
blocks side by side counts as a switch, a stay ends at a hole, and the
lags of the level sequence are times apart, pairing only blocks that far
apart in the take.

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
`rate_change`, `amplitude_change` and `beat_error_change` in the JSON),
Im (the largest, excluding 12H, as in NIHS 93-10), Im\* (over all
positions), Ie (change in X over the positions measured in both).

## Findings

The findings are conditions found in the sound of one recording, each
with the figures that show it. None judges the watch: the report has no
tolerances, limits or advice. Published tolerances (Witschi's, COSC's,
the movement maker's) and what a figure means for the movement are in
the watchmaker skill, each with its source.

| Code | Title, e.g. | Listed when |
| --- | --- | --- |
| `no_regular_beat` | No regular beat found (timing scatter 8.6 ms, 6.9% of a beat) | the median timing scatter is 2% of the beat period or more |
| `unlock_unreliable` | Unlock found in 45% of windows | fewer than 60% of the 2 s amplitude windows find the unlock on both sides; amplitude and the beat error from the unlock rest on those windows |
| `clipping` | Clipped on 3.2% of beats | over 1% of the measured beats have a clipped sample in their template window, as `analyze` counts them |
| `measurement_short` | 25 s measured | under 40 s measured after settling (D40e's "≥40 s measurement time", about p. 34) |
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

`input.settings.analysis_only` is null, or for a take kept as its
analysis alone its `note` and `measured_by` (see
[A take kept as its analysis alone](#a-take-kept-as-its-analysis-alone)).

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
