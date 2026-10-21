# Seiko and Miyota train-period research

Research date: 2026-10-19. This contribution audits the **182 existing Seiko
and Miyota catalogue rows** and adds source-backed escape-wheel periods to
**42 canonical rows**, including newly separated and newly added variants.
The complete before-state audit, coverage list, source URLs, SHA-256 hashes
and locations of counted illustrations are in
[research.json](assets/seiko-miyota/research.json).

**This is a partial train specification, not a complete tooth census.**
The new result is a **5-second escape-wheel revolution** for the explicitly
covered calibres. Third-wheel and barrel periods, and escape-pinion leaf
counts, remain unknown. No watch was measured on the bench for this research.

## Counts and calculation

| Escape assembly | Evidence for outer tooth count | Teeth | Beat rate | Period |
| --- | --- | ---: | ---: | ---: |
| Miyota 032-106 | [8215 manufacturer exploded view](https://miyotamovement.com/uploads/product/product_moQeV8byx0JPUZStgs.jpg) | 15 | 21,600 bph | 5 s |
| Miyota 032-991 | [9015 manufacturer exploded view](https://miyotamovement.com/uploads/product/product_cAzkTHoO04Nhn9rI86.jpg) | 20 | 28,800 bph | 5 s |
| TMI 0251 383 | [NH3 technical guide](https://www.timemodule.com/upload/category/24/technical_guide/NH35_TG.pdf), PDF p11, printed p10 | 15 | 21,600 bph | 5 s |
| Seiko 0251 300 / 0251 013 | [7S26C/7S36C technical guide](https://seikoserviceusa.com/uploads/datasheets/7S26C_36C.pdf), p6, shared illustration with both part labels | 15 | 21,600 bph | 5 s |
| Seiko 0251 283 | [Seiko 6R35A technical guide, distributor-hosted copy](https://cousinsuk.com/pdf/skus/9509_6r35.pdf), p7 | 15 | 21,600 bph | 5 s |

These are **manual counts of tooth tips in manufacturer illustrations**,
not explicit numeric specifications printed by the manufacturers. The source
images can be revisited using the crop rectangles in the research manifest.
Illustrations are weaker evidence than a counted physical specimen; an
actual revision with different teeth would supersede the relevant entry.
The pinions are not sufficiently exposed to count reliably. A first-person
[9015 service walkthrough](https://www.watchrepairtalk.com/topic/19979-miyota-calibre-9015-service-walkthrough-review-and-eta-2824-2-comparison/)
provides a physical 20-tooth escape-wheel photograph (IMG_6782) as an
independent visual check of the 9015 drawing.

For these lever escapements, one tooth pitch advances per full balance
oscillation, or two beats:

```text
T_escape = 7200 × escape_teeth / beats_per_hour
21,600 bph: 7200 × 15 / 21600 = 5 seconds (30 beats)
28,800 bph: 7200 × 20 / 28800 = 5 seconds (40 beats)
T_balance = 7200 / beats_per_hour
```

The shared five-second period does **not** imply identical tooth counts,
pinion counts, escapement geometry, lift angle or interchangeable parts.

## Coverage and boundaries

Only the escape count is transferred by part identity. Every covered group
also needs its documented frequency. The manifest lists the exact current
manufacturer parts-list URL for each Miyota variant, rather than treating
an entire numbered family as mechanically identical.

| Group | Covered identities | Basis and limitations |
| --- | --- | --- |
| Miyota 82xx | 8215, 8205, 8217, 821A, 820A; 82S0, 82S5, 82S7, 8N24, 8N40 | Individual current parts lists use 032-106; the [older Miyota combined guide](https://om-mechanics.com/brochures/Miyota%208205%2C%208215%2C%20820A%2C%20821A.pdf) supplies 821A/820A. Hand layout and centre/fourth parts vary. |
| Miyota 90xx | 9015, 9039, 90S5, 9075 | Individual manufacturer lists use 032-991. The GMT 9075 has a separate row. Fourth-wheel and cannon-pinion part differences are not ignored. |
| Miyota 91xx | 9100, 9110, 9120, 9122, 9130, 9132 | Current lists plus [Miyota's combined guide](https://okta.ua/files/pdf/14902832299122.pdf), p1 specifications and p8 parts columns, establish 032-991 and 28,800 bph. All six existing exact rows are updated. |
| TMI NH3 | NH34A, NH35A, NH36A, NH37A, NH38A, NH39A, and their suffix-free names | Version 04 NH3 guide explicitly covers these six; 0251 383. |
| TMI NH7 | NH70A, NH71A, NH72A, and suffix-free names | [NH7 guide](https://www.timemodule.com/upload/category/29/technical_guide/NH70_TG.pdf), PDF pp2 and 6: same 0251 383 and frequency; finish differences are listed. |
| Seiko 4R | 4R35A/B, 4R36A, 4R37A, 4R38A, 4R39A | Each revision's own guide identifies 0251 300. Suffix-free 4R35 resolves to B; explicit A remains separate. |
| Seiko 7S | 7S26A/B/C, 7S36A/B/C, generic 7S26/7S36; 7S25C and 7S35C | A, B and C guides identify the 0251 300 / 0251 013 pair. The C-only 25/35 rows are not broadened to other revisions. |
| Seiko 6R15 | 6R15A/B/C/D and generic 6R15 | [A/B guide](https://seikoserviceusa.com/uploads/datasheets/6R15AB.pdf) and [C/D guide](https://seikoserviceusa.com/uploads/datasheets/6R15CD.pdf) identify 0251 300. Barrel revisions are not assigned a common running period. |
| Seiko 6R35 and later listed models | 6R35/6R35A, 6R31A, 6R38A, 6R54A, 6R55A, with suffix-free aliases for the latter four | Each named guide lists 0251 283 at 21,600 bph. These do not inherit the 6R15 escape assembly or an assumed barrel period. |

The full manufacturer links for 4R, 7S and newer 6R revisions are in the
manifest, with file hashes to identify exactly which versions were read.

The broad `6R` and `7S` aliases are removed. In particular, 6R20/21/24 and
other frequencies must not accidentally acquire this 21,600 bph train.
The NH35 row no longer stands in for an unqualified Seiko 4R35/36; those
names resolve to the corresponding Seiko records with their own sources.

Miyota 8200 is separated from the old 8215 alias. Its existing limited
hand-period and lift-angle data remain available, but the new escape count
is not silently attached without a calibre-specific manufacturer source.
8315, 82S3, NH05, vintage 6309/7002/7009 and the other unverified historical
entries receive no new escape count. A 032-106 parts list was found for
82S3, but its frequency evidence was not completed in this contribution.
These are documented gaps, not assertions that their escape periods differ.

## Other periods and corrections

- **Seconds:** a hand-carrying arbor rotates in 60 s. The direct-seconds
  entries use `fourth wheel`. Miyota's indirect centre-seconds variants use
  `seconds pinion`; indirect drive alone establishes neither equality nor
  inequality with the fourth wheel's period. The old 8215 note incorrectly
  asserted that the periods must differ.
- **Minutes:** the established minute-hand/cannon arbor rotates in 3600 s
  during normal running. Seiko's centre-wheel/cannon assembly and Miyota's
  conventional 82xx centre-wheel layout retain their hour period. This is a
  hand-layout constraint, not something derivable from beat rate alone.
- **Miyota 90xx/91xx:** the manufacturer's parts lists distinguish the
  off-centre centre wheel from the cannon pinion with its driving wheel.
  The 9015 service walkthrough shows the separate drive and third-wheel
  pinions. The unsupported `centre wheel = 3600 s` entry is replaced by
  `cannon pinion = 3600 s`; the off-centre wheel's period is left unknown.
- **Seiko reduction wheels:** the first and second reduction wheels in the
  NH/7S guides belong to automatic winding. They are not unnamed extra
  stages in the going train. The old NH35 note is corrected.
- **Lift:** existing Miyota 8215 (49°) and 9015 (51°) settings are retained.
  The 91xx combined manufacturer sheet explicitly supplies 51°. New Miyota
  variants without checked lift-angle evidence are left unspecified.
  Seiko 4R35A explicitly specifies **52°**, while B and the other covered
  later 4R entries specify **53°**. The C-series 7S guide explicitly gives
  53°, replacing the generic row's old claim that no Seiko source was found.

There are also typographical errors in otherwise useful primary sources:
the 4R37A and 4R38A/39A guides print 21,600 per hour alongside “5 beats per
second”. The hourly figure equals **6** beats/s and is used here; the
parenthetical figure is not a second supported operating frequency.

## Why the third wheel and barrel remain blank

Manufacturer parts catalogues identify parts and assembly order but do not
publish the required complete tooth/leaf counts. Teardown photographs and
parts-vendor photographs were examined for NH/7S and Miyota 9015. Wheel
rims are sometimes countable, but the needed mating pinion leaves are
hidden, blurred or shown edge-on. Reusing an illustration's guessed leaf
count would turn a partial result into false precision.

A public [gear-train calculator table](https://uscalculators.com/horology/gear-train-bph-calculator/)
was rejected as evidence. Its suggested counts fail its own hour-centre
assumption and the claimed frequencies:

```text
Table's 2824/9015 example:
(80/10) × (75/8) × (70/7) × 15 × 2 = 22,500 bph, not 28,800
Table's NH35/82xx example:
(80/10) × (64/6) × (63/7) × 21 × 2 = 32,256 bph, not 21,600
```

Neither that table nor secondary movement summaries supply any new count
in this contribution. Power reserve, turns needed to wind a spring, and
automatic-winding reduction ratios do not establish the barrel drum's
running period.

To close the remaining gaps, obtain a face-on view of the barrel teeth,
centre wheel and its pinion, third wheel and **each** pinion, fourth wheel
and its input pinion, and escape pinion. For indirect-seconds Miyotas,
include the centre-seconds pinion and identify its driving wheel. For the
9015 family, include the cannon driving wheel and confirm which third
pinion meshes with it. Count both sides where stacked wheels obscure a
pinion. Record the exact calibre and revision; then check every adjacent
ratio against the hand periods and beat rate before extending coverage.

## Validation and reproducibility

Validation checks the escape-period equation, the ledger's exact
coverage against the database, runtime alias boundaries, retained 4R35A/B lift
differences, the Miyota indirect-seconds distinction and the 9015 cannon
correction. The original PDFs and third-party photos remain at their
linked sources; the repository stores the research facts, hashes and
count locations rather than republishing whole manuals or other people's
photographs. These hashes establish the bytes consulted, not their future
availability or a manufacturer's endorsement of the counted result.
