# Patek 240 clone: photographed train-wheel counts

Peter Hudson supplied six microscope photographs on 2026-10-21, identifying the
specimen as a Patek 240 clone. On 2026-10-22 he identified its maker as
Shanghai. No existing clone record was found in the catalogue, so the new entry
is **Patek 240 Clone**, with maker **Shanghai (clone)**, the name the movement
commonly goes by; "PP240 Clone" also finds it. These photographs
do not establish genuine Patek Philippe 240 counts or specifications.

## Direct counts

| Component | Wheel teeth | Pinion leaves / additional section |
| --- | ---: | --- |
| Barrel drum | 96 | — |
| Second wheel | 75 | near section 15; far section 16 |
| Third wheel | 64 | 10 |
| Fourth wheel | 72 | 8 |
| Escape wheel | 20 | 8 |

The second-wheel side profile shows two distinct steel sections. The near
section (farther from the brass wheel) has **15 leaves**; the far section
(next to the brass wheel) has **16 leaves**. Peter Hudson identified the error
in the initial near-section overlay on 2026-10-21. Recounting the foreground
outline confirms 15; the original overlay mixed tips from the two sections.
The corrected figure and coordinates replace that erroneous 16-leaf overlay.

Peter Hudson subsequently checked the assembled mechanism and confirmed that
the **far 16-leaf section meshes with the barrel**, while the **near 15-leaf
section meshes with the dial-side motion works**. The exact motion-works mating
wheel has not been identified. The database's single input-pinion field records
16; the separate 15-leaf section is preserved here and in the catalogue notes.

## Ratios and confirmed beat rate

For each mesh, the driven period is the driver period multiplied by driven
pinion leaves / driver wheel teeth. The photographed train gives:

- Second to third: speed increase `75/10 = 7.5`.
- Third to fourth: speed increase `64/8 = 8`.
- Second to fourth: speed increase `7.5 × 8 = 60`.
- Fourth to escape: speed increase `72/8 = 9`.
- Barrel to second: `96/16 = 6`, using the confirmed far-section mesh.

Peter Hudson confirmed on 2026-10-21 that this clone beats at **21,600 vph**.
That specimen-specific confirmation supplies the time anchor. For a lever
escapement, one complete balance oscillation advances the escape wheel one
tooth, so `20 × 7200 / 21600 = 20/3 s` per escape-wheel revolution.
Working backwards through the counted meshes gives:

| Component | Derived period |
| --- | ---: |
| Barrel drum | 21,600 s = 6 h |
| Second wheel | 3,600 s = 60 min |
| Third wheel | 480 s = 8 min |
| Fourth wheel | 60 s |
| Escape wheel | 20/3 s = 6⅔ s |
| Balance | 1/3 s |

The calculations are `T_fourth = (20/3) × 72/8 = 60 s`,
`T_third = 60 × 64/8 = 480 s`, `T_second = 480 × 75/10 = 3600 s`,
and `T_barrel = 3600 × 96/16 = 21600 s`. The balance period is
`7200/21600 = 1/3 s`.

The second wheel's one-hour period and fourth wheel's one-minute period are
therefore derived results; no independent hand-period observation is needed.
The database's centre-wheel slot represents this second wheel. The database
stores all six derived periods. The photographed counts and the bench-confirmed
roles of the two second-wheel sections are preserved in the evidence and notes. Lift angle and
seconds-hand arrangement remain unspecified. Barrel drum period is a running
rotation period, not the movement's power reserve. The genuine calibre's
specifications were not used as the time anchor.

## Recountable evidence

The six [original photographs](assets/train-wheels/pp240-clone/originals/manifest.json)
are preserved byte for byte, with supplied filenames and SHA-256 hashes.
[Tooth-tip coordinates](assets/train-wheels/pp240-clone/tooth-tip-coordinates.json)
use a 1824 × 1368 display of each original. Outer silhouettes were sampled in
polar coordinates, then every proposed tooth was checked against the numbered
image. Pinion tips were counted individually. Labels and crops are analytical
overlays; the original photographs remain unchanged.

| Photograph | Numbered evidence |
| --- | --- |
| Barrel | [96 teeth](assets/train-wheels/pp240-clone/barrel-rim-96.jpg) |
| Second wheel | [75 teeth](assets/train-wheels/pp240-clone/second-wheel-rim-75.jpg), [near section 15](assets/train-wheels/pp240-clone/second-wheel-near-section-15.jpg), [far section 16](assets/train-wheels/pp240-clone/second-wheel-far-section-16.jpg) |
| Second wheel profile | [Unmodified profile](assets/train-wheels/pp240-clone/originals/second-wheel-profile.jpg) |
| Third wheel | [64 teeth](assets/train-wheels/pp240-clone/third-wheel-rim-64.jpg), [10 leaves](assets/train-wheels/pp240-clone/third-wheel-pinion-10.jpg) |
| Fourth wheel | [72 teeth](assets/train-wheels/pp240-clone/fourth-wheel-rim-72.jpg), [8 leaves](assets/train-wheels/pp240-clone/fourth-wheel-pinion-8.jpg) |
| Escape wheel | [20 teeth](assets/train-wheels/pp240-clone/escape-wheel-rim-20.jpg), [8 leaves](assets/train-wheels/pp240-clone/escape-wheel-pinion-8.jpg) |
