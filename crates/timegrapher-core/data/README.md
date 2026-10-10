# Train-wheel periods of common calibres

`train-wheels.json` lists common mechanical calibres with the time each
wheel of the going train takes to turn once. A fault on a wheel (a bent
tooth, an eccentric wheel, a rubbing seconds hand) repeats once per turn,
so a periodic change in rate or amplitude can be named once these periods
are known. The table is read by `timegrapher_core::calibres`.

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
  calls a 20-tooth Swiss lever its standard, SJX reports that Chronergy
  kept the old tooth count, and every 28,800 vph watch recorded in this
  project (Rolex 3235, Dandong 3235 and 4130, Patek 324 clone, Peacock
  SL1258) shows a rate cycle of exactly 5.00 s locked to 40 beats. So the
  Rolex calibres and the clones are entered with 20 teeth: "inferred" for
  the genuine Rolex calibres, "measured" for the clones recorded here.
  The 2892-A2, 7750, Miyota 9015 and the Seiko 21,600 vph calibres have
  no escape wheel entry because no count was found.
- **Barrels.** Rolex 3135: 5 h, from SJX's 5:1 barrel to minute ratio.
  Rolex 3235: 22,050 s, from SJX's 98:16 ratio read as barrel teeth to
  centre-pinion leaves (provisional). Rolex 4130: 7 h, from Caliber Corner.
- **Clones.** A clone is given the original's train only where its genuine
  parts are known to fit (Dandong 3235 and 4130). 7750-based "4130" clones
  and the Shanghai 4130 and VR3235, whose escapements are not to the
  original's specification, are not given the original's escape wheel.
- **Lift angles** are what sources give. Several are widely used rather
  than published (Rolex 3135 at 52°, the 3235 at 55° with a reported
  revision to 53°, the 4130 at 52° or 55°). The Miyota 8215 is 49°, not
  the 52° timegrapher default.

## Not found

Third-wheel and barrel tooth counts for every calibre except as above;
escape wheel counts for the 2892-A2, 7750, Powermatic 80, Omega
co-axials, Seiko and Miyota; any published data for the Dandong 324 or the
Peacock SL1258; lift angles for the Sellita, Powermatic 80, Seiko 7S26,
Seagull and Patek 324 calibres. Corrections with a source are welcome.

Built from manufacturer documents, technical guides, parts catalogues,
watchmaking references and forum teardowns, cited per entry; nothing was
taken from any other timegrapher program.
