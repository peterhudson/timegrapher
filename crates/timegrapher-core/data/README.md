# Train-wheel periods of common calibres

`train-wheels.json` lists common mechanical calibres with the time each
wheel of the going train takes to turn once. A fault on a wheel (a bent
tooth, an eccentric wheel, a rubbing seconds hand) repeats once per turn,
so a periodic change in rate or amplitude can be named once these periods
are known. The table is read by `timegrapher_core::calibres`.

Only lever escapements are listed. Co-axial calibres (Omega 8500, 8800
and their kin) and other non-lever escapements are left out: the
measurement assumes a Swiss lever's unlock and drop.

## Shape

A JSON array, one object per calibre:

| Field | Meaning |
| --- | --- |
| `calibre` | Name shown to the user, e.g. "ETA 2824-2" |
| `maker` | Maker, or "(clone)" for a copy |
| `family` | Other names that match: sister calibres with the same train, the names this project's session files use (e.g. "3235", "4130 (clone)") |
| `bph` | Beats per hour |
| `escape_teeth` | Escape wheel teeth, only where there is evidence |
| `lift_angle_deg` | Lift angle, only where a source gives one |
| `seconds` | Where the seconds hand sits and how it is driven |
| `wheels` | `name` (lower case), `period_s` (one full turn, seconds), and where known `teeth`, `pinion` (leaves on the same arbor) and `basis` (how the period is known) |
| `sources` | Where each figure came from |
| `confidence` | The best basis in the entry: "derived from tooth counts", "measured" (from this project's recordings), or "derived from the beat rate" |
| `notes` | What is inferred, where sources disagree, what is missing |

## How the periods were worked out

- **balance**: one full swing is two beats, `7200 / bph` seconds.
- **escape wheel**: a lever escape wheel moves one tooth per swing, so
  `teeth × 7200 / bph`. At 28,800 vph a 20-tooth wheel turns in 5.0 s and
  a 15-tooth wheel in 3.75 s; at 21,600 vph a 15-tooth wheel turns in 5.0 s.
- **fourth wheel** (or **seconds wheel** / **seconds pinion** where the
  seconds are driven indirectly and the fourth wheel's own period is not
  known): 60 s when it carries the seconds hand.
- **centre wheel**: 3600 s where it sits at the centre and carries the
  minute hand. Where the great wheel is off centre (ETA 2824-2, Valjoux
  7750) its period is not known, and the **cannon pinion**, which carries
  the minute hand, is listed at 3600 s instead.
- **third wheel** and **barrel**: only from tooth counts or a published
  ratio: third = fourth × third-wheel teeth / fourth-pinion leaves, barrel
  = centre × barrel teeth / centre-pinion leaves.

Where both a wheel's teeth and the next pinion's leaves are given, the
unit tests check the two periods agree. A wheel whose period is not known
is left out rather than guessed.

## How far to trust it

Tooth counts are rarely published. Only the Unitas 6497-2 / 6498-2 has a
full published train (escape 15 on a 10-leaf pinion, fourth 120/8, third
60/10, centre 80). Everything else rests on the beat rate and the layout,
plus these:

- **20-tooth escape wheels at 28,800 vph.** The ETA 2824-2's escape wheel
  is documented with 20 teeth. Rolex's own escapement patent (US8529122)
  uses a standard 20-tooth Swiss lever as its baseline (its own design
  has 24 teeth), SJX reports that Chronergy kept the old tooth count, and every 28,800 vph watch recorded in this
  project (Rolex 3235, Dandong 3235 and 4130, Patek 324 clone, Peacock
  SL1258) shows a rate cycle of exactly 5.00 s locked to 40 beats. So the
  Rolex calibres and the clones are entered with 20 teeth: "inferred" for
  the genuine Rolex calibres, "measured" for the clones recorded here
  (the Dandong 3235 recorded here carries a genuine escape wheel).
  The 2892-A2, 7750, Miyota 9015 and the Seiko 21,600 vph calibres have
  no escape wheel entry because no count was found.
- **Barrels.** Rolex 3135: 5 h, from SJX's 5:1 barrel to minute ratio.
  Rolex 3235: 22,050 s, from SJX's 98:16 ratio read as barrel teeth to
  centre-pinion leaves (provisional). Rolex 4130: 7 h, from Caliber Corner.
- **Clones.** A clone is given the original's train, escape wheel
  included, only where it is described as a true clone that takes genuine
  parts (DD3235 and DD3285, JH3235, VR3135, DD4130), or whose maker says
  nearly all parts interchange (DD4131). Design clones and hybrids
  (SH3135, SA3135, VR3235, SH3285, SH4131) and 7750-based "4130" clones
  get only what follows from the beat rate and layout. Most clone facts
  come from two guides by one author on a watch forum (one on clone
  movements, one on lift angles) and teardown threads, cited by URL per
  entry; the escape wheel of a clone resting on them alone is marked
  inferred.
- **Lift angles** are what sources give. Rolex, AP and Patek publish
  none, so theirs are the figures timegrapher users work with (Rolex 31xx
  and 4130 52°, 32xx and 4131 55° with a reported Rolex revision of the
  32xx to 53°, AP 3120 53°, AP 4302 52°, Patek 324 52°). The SH4131
  has a T-shaped pallet fork, not the 4131's; it is set at 52°, as the
  31xx, whose fork geometry it resembles (a bench setting, not a
  published figure). The Miyota 8215 is 49°, not the 52°
  timegrapher default.

## Not found

Third-wheel and barrel tooth counts for every calibre except as above;
escape wheel counts for the 2892-A2, 7750, Powermatic 80, AP, Seiko and
Miyota calibres; the AP clones' trains; any published data for the
Dandong 324 or the Peacock SL1258; lift angles for the Sellita, Powermatic 80, Seiko 7S26,
Seagull and Patek 324 calibres. Corrections with a source are welcome.

Built from manufacturer documents, technical guides, parts catalogues,
watchmaking references and forum teardowns, cited per entry; nothing was
taken from any other timegrapher program.
