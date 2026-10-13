# Fault signatures

How common escapement faults show up in the sound of a beat, and the
measurements timegrapher reports that describe that sound. The fault list
and the waveform shapes follow Witschi, *Testing methods for mechanical
watches* (2025), "Fault Detection". Witschi's own note applies: the
examples are not exhaustive, and similar patterns can have different
mechanical causes. The features listed are this project's reading of
Witschi and have not been checked against recordings of watches with
confirmed faults.

Each beat has three sounds: **1** unlock, **2** impulse, **3** drop. Beats
alternate between the two pallet stones, so a fault confined to one stone
shows on every other beat only. Amplitude is measured from 1 to 3.

## Scope (shape of one beat)

| Fault | What the waveform shows | Feature to measure |
| --- | --- | --- |
| Escapement fitting weak (shallow lock) | Sounds 1 and 2 close together, 1 small | Short 1→2 interval relative to this calibre; low level of 1 |
| Escapement fitting strong (deep lock) | Three well-separated sounds, a long gap 1→2 | Long 1→2 interval |
| Unlocking too strong | Sound 1 as loud as or louder than 3 | Level ratio 1:3 high |
| Additional friction in the escapement | Sounds 1 and 2 loud, broad and merged | Energy between the sub-peaks high; 1 and 2 not separable |
| Safety pin (dart) touching the roller | Small clusters of sound before 1 and after 3 | Extra events outside the 1→3 span, a few ms away, as bursts |
| Too little slot / impulse-pin clearance | An extra small sound just after the drop | Secondary event in the drop's tail |
| Weak amplitude | Long gap from 1 to 3 | Amplitude itself |
| Too much axial end shake in the balance pivot | Sounds appear doubled and their spacing varies from beat to beat | Per-beat sub-event timing with two clusters; doubled peaks |
| Impulse pin touches the fork horn (knocking) | A single sharp extra sound before 1 and another after 3 | Isolated extra events either side of the beat |
| Damaged or worn balance-staff pivot | Raised continuous noise between beats | Noise floor between beats well above the silence before the unlock |
| Grazing balance wheel or hairspring | Sustained rubbing noise after the drop, ending in another burst | Long post-drop energy; periodic rubbing bursts |
| Escape tooth lands directly on the impulse plane | No distinct unlock; a small sound where the impulse should be, then the drop | Missing sound 1; only two events |

### What `analyze` reports

`analyze` measures the shape and reports it as figures; it no longer
names faults. The `shape` block of `analyze --json` (and the `Shape` lines
of its text) gives, for the Tick (`even`) and the Tock (`odd`):

| Measure | What it is |
| --- | --- |
| `windows_measured` | 2 s windows in which the side's shape was measured |
| `share_with_sound_1` | share of those windows in which sound 1 (the unlock) was found |
| `share_with_sound_2` | share in which sound 2 (the impulse) was found apart from sound 1 |
| `i12_ms`, `i13_ms` | sound 1 to sound 2, and sound 1 to sound 3, ms (medians over the windows) |
| `ratio13` | level of sound 1 over the level of sound 3 |
| `tail_ratio` | mean level 3 to 15 ms after the drop over the drop's level |

and, for the beat error measured from the unlock,
`beat_error_first_quarter_ms` and `beat_error_last_quarter_ms`: its median
over the first and the last quarter of the 2 s windows
(`beat_error_quarter_windows` in each). `timegrapher shape` gives the full
set of measures.

Reading these figures against the faults above, and Witschi's timing
diagram patterns, is interpretation: it lives in the
[watchmaker skill](../skills/watchmaker/SKILL.md), not in the program.

## Calibre 3235 notes

The first recordings (dial up, 2026-10-08) show the rate moving between
about 0 and +50 s/d over tens of seconds, and the amplitude varying with a
period close to 60 s, the period of the fourth wheel. Longer recordings are needed to confirm it.
