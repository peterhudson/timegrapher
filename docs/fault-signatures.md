# Fault signatures

How common escapement and gear-train faults show up, and the measurements
timegrapher will use to flag them. The fault list and the waveform shapes
follow Witschi, *Testing methods for mechanical watches* (2025),
"Fault Detection". Witschi's own note applies: the examples are not
exhaustive, and similar patterns can have different mechanical causes.
The suggested features are this project's interpretation and still need
checking against recordings of watches with confirmed faults.

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

## Diagram (beat timing over time)

| Fault | What the diagram shows | Feature to measure |
| --- | --- | --- |
| Good condition | One straight line | — |
| Beat error too large (about 3 ms) | Two parallel lines | Beat error |
| Rate needs adjusting | A steep line | Rate |
| Large differences between vertical positions | Rate changes a lot between vertical positions | Positional spread |
| Vertical vs horizontal difference | Rate differs between dial up and pendant positions | V–H index |
| **Large but regular rate variations** | Line waves regularly: a gear-train fault | Periodicity of rate (Feature 1) |
| Very irregular rate | Scattered, wandering line; usually low amplitude, needs service | Jitter, amplitude |
| Balance knocks intermittently (overbanking) | Line jumps in steps; amplitude above 330° | Amplitude > 330°, step changes in residual |
| Balance knocks continuously | Line breaks into doubled segments | Persistent double beats |
| Escape wheel out of round | Smooth wave, one cycle per escape-wheel turn (15–21 teeth) | Period = teeth × 2 beats (3.75 s for 15 teeth at 28,800) |
| Pallet fork poorly fitted or dry (gummed) | Points scatter below the line | One-sided late outliers |
| Hairspring rubbing | Two lines that drift apart | Beat error changing over time |
| Slow oscillation after a position change | A slow curve after moving the watch | Settling time after a position change |

## Calibre 3235 notes

The first recordings (dial up, 2026-10-08) show the rate moving between
about 0 and +50 s/d over tens of seconds, and the amplitude varying with a
period close to 60 s. That matches "large but regular rate variations" with
the period of the fourth wheel. Longer recordings are needed to confirm it.
