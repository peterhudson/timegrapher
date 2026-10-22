# Dandong 324 SC and Peacock SL1258 train-wheel counts

Peter Hudson supplied eight microscope photographs from a **Dandong 324 SC
clone** on 2026-10-21. All train wheels are now photographed, including the
fourth wheel supplied after the initial analysis. The counts give a
**600-second third wheel**, **60-second fourth wheel**, **5-second escape
wheel** and **18,600-second (5 h 10 min) barrel drum**.

Peter confirms that **Peacock SL1258 and variants retaining this train**
share these counts. The SL1258 database entry records this as a
contributor-confirmed family transfer, not an independently photographed
SL1258 specimen. No counts are transferred to genuine Patek Philippe 324
or other Patek/clone families. No new unnamed variant aliases are invented.

## Counts and periods

| Component | Wheel teeth | Pinion leaves | Seconds per turn |
| --- | ---: | ---: | ---: |
| Barrel drum | 93 | — | 18600 |
| Second wheel | 60 | 18 | 3600 |
| Third wheel | 90 | **10 on each face** | 600 |
| Fourth wheel | **84** | **9** | **60** |
| Escape wheel | 20 | **7** | 5 |
| Separate centre-seconds pinion | — | 9 | 60 |
| Cannon wheel / cannon pinion | 60 | 28 (motion works) | 3600 |
| Balance | — | — | 0.25 |

The wheel and pinions on each rigid arbor share a period. The barrel period
is the drum's running period, not the winding-arbor speed or power reserve.
The cannon assembly's 28-leaf pinion belongs to the motion works; it is
not the receiving gear in the third-to-cannon mesh.

## Meshes and independent checks

Peter supplied the 28,800 beats/hour rate, one-minute centre seconds,
one-hour cannon wheel and the two branch connections from the third wheel:
its large rim drives centre seconds, and its pinion drives the cannon
wheel. Both third-wheel faces show ten leaves. The ordinary going-train
path and the stated branches give:

```text
barrel 93 → second pinion 18
second rim 60 → third input pinion 10
third rim 90 → fourth pinion 9
fourth rim 84 → escape pinion 7

third rim 90 → separate centre-seconds pinion 9
third pinion 10 → cannon wheel 60
```

For a meshing driver A and driven gear B, `T_B = T_A × Z_B / Z_A`.
The third-wheel period is independently fixed by each hand:

```text
T_third = 60 × 90/9       = 600 s   [centre-seconds constraint]
T_third = 3600 × 10/60    = 600 s   [cannon constraint]

T_second = 600 × 60/10    = 3600 s
T_barrel = 3600 × 93/18   = 18600 s
T_fourth = 600 × 9/90     = 60 s
T_escape = 60 × 7/84     = 5 s
```

A lever escape wheel advances one tooth pitch per two beats. Its counted
20 teeth at 28,800 beats/hour independently give
`20 × 7200/28800 = 5 s`, agreeing with the train calculation.

### Fourth-wheel confirmation

Before the fourth-wheel photograph, the other counts and speeds constrained
only `fourth teeth / fourth pinion leaves = 28/3`. The integer pairs 56/6,
84/9 and 112/12 all satisfy that relation, with different fourth-wheel
periods. The centre-seconds pinion is a separate arbor, so its 60-second
period did not establish the fourth-wheel period.

The additional photograph directly shows **84 teeth and nine leaves**,
resolving that ambiguity. Neither fourth-wheel count remains inferred.
The fourth wheel and the separate centre-seconds pinion both happen to
rotate once per minute; they remain separate database components.

## Database representation and scope

The two existing canonical entries, **Dandong 324** (including DD324 and
Dandong 324 S C) and **Peacock SL1258**, receive the complete train. All
other catalogue rows are unchanged.

Following the existing 31xx/2824 conventions, the `centre wheel` slot
represents the second wheel, here at 3600 seconds. The `cannon pinion`
entry represents the photographed 60-tooth cannon wheel and its 28-leaf
motion-works pinion. Its one-hour period is established separately.
`third wheel.pinion = 10` records its input pinion; the equally counted
10-leaf cannon-driving section is documented in the basis and here.

The old generic `seconds wheel` entry is clarified as `seconds pinion`,
with the photographed nine leaves. The newly established fourth wheel
is a separate entry. Earlier bench observations of approximately 60-second
and exactly 5-second cycles are retained as historical sources; they do
not distinguish two separate components with the same period.

Lift-angle values are unchanged. DD324 retains its existing **52°** forum
source; SL1258 remains **unspecified** because a 52° bench setting is not a
verified specification. Train counts do not establish lift angle or parts
interchangeability.

## Evidence and reproducibility

The [eight original JPEGs](assets/train-wheels/dd324/originals/) retain their
original bytes. The [manifest](assets/train-wheels/dd324/originals/manifest.json)
records supplied names, normalized repository names and SHA-256 hashes.
The source spellings `Centre Secodns Pinion.JPG` and `Third Wheel Botom.JPG`
are preserved in the manifest, with corrected names on repository assets.

[Tooth-tip coordinates](assets/train-wheels/dd324/tooth-tip-coordinates.json)
use a 1824 × 1368 viewing frame. Rim counts were obtained from radial colour
or brightness boundaries and cross-checked across four thresholds; both
third-wheel views independently give 90. Escape teeth and pinion leaves
were counted visually. Numbered figures plot those coordinates over the
photographed pixels; pinion figures are enlarged details. The originals
remain the authoritative image evidence. No tooth geometry was generated.

### Going-train rims

![Barrel: 93 teeth](assets/train-wheels/dd324/barrel-93-teeth.jpg)

![Second wheel: 60 teeth](assets/train-wheels/dd324/second-wheel-60-teeth.jpg)

![Third wheel top: 90 teeth](assets/train-wheels/dd324/third-wheel-top-90-teeth.jpg)

![Third wheel bottom: 90 teeth](assets/train-wheels/dd324/third-wheel-bottom-90-teeth.jpg)

![Fourth wheel: 84 teeth](assets/train-wheels/dd324/fourth-wheel-84-teeth.jpg)

![Escape wheel: 20 teeth](assets/train-wheels/dd324/escape-wheel-20-teeth.jpg)

### Pinions and hand-drive branches

![Second pinion: 18 leaves](assets/train-wheels/dd324/second-wheel-18-leaves.jpg)

![Third top pinion: 10 leaves](assets/train-wheels/dd324/third-wheel-top-10-leaves.jpg)

![Third bottom pinion: 10 leaves](assets/train-wheels/dd324/third-wheel-bottom-10-leaves.jpg)

![Fourth pinion: 9 leaves](assets/train-wheels/dd324/fourth-wheel-9-leaves.jpg)

![Escape pinion: 7 leaves](assets/train-wheels/dd324/escape-wheel-7-leaves.jpg)

![Separate centre seconds: 9 leaves](assets/train-wheels/dd324/centre-seconds-pinion-9-leaves.jpg)

![Cannon wheel: 60 teeth](assets/train-wheels/dd324/cannon-wheel-60-teeth.jpg)

![Cannon motion-works pinion: 28 leaves](assets/train-wheels/dd324/cannon-wheel-28-leaves.jpg)
