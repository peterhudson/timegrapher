# Calibres: lift angles and train-wheel periods

`train-wheels.json` lists mechanical calibres with their beat rate and
lift angle and, where known, the time each wheel of the going train
takes to turn once. A fault on a wheel (a bent
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
| `family` | Other names that match: sister calibres with the same train, the names this project's session files use (e.g. "3235", "DD4130") |
| `former_names` | Names the row had in earlier versions of the table (the 97 variant rows were first named with "vph"); they still match, with or without the maker, but are not searched or shown in the app. Left out when there are none |
| `common` | `true` for common and modern calibres, the short list the app offers first; uncommon and vintage ones (most of the imported list) leave it out and appear when the list is expanded or searched |
| `bph` | Beats per hour, where a source gives it |
| `escape_teeth` | Escape wheel teeth, only where there is evidence |
| `lift_angle_deg` | Lift angle, only where a source gives one |
| `seconds` | Where the seconds hand sits and how it is driven |
| `wheels` | `name` (lower case), `period_s` (one full turn, seconds), and where known `teeth`, `pinion` (leaves on the same arbor) and `basis` (how the period is known); empty when no wheel's period is known |
| `sources` | Where each figure came from |
| `confidence` | The best basis in the entry: "derived from tooth counts", "measured" (from this project's recordings), "derived from the beat rate", or "published list" (an imported row, below) |
| `notes` | What is inferred, where sources disagree, what is missing |

## Makers' other names

`makers.json` lists, for a `maker` as the table writes it, whether it is
one of the big makers listed first (`common`), and the other
names people search by: abbreviations ("AP" for Audemars Piguet, "DD" for
Dandong), older names, and the short marks calibres carry ("AS" for
A. Schild, "FHF" for Fontainemelon). Each carries its source; for the
imported makers that is the list that writes the maker that way. The
clone prefixes (DD, SH, JH, VR, PTS) appear in calibre names and parts
listings but in no neutral reference list.

## The imported lift-angle lists

Most rows (`confidence` "published list") come from three public lists
of lift angles, imported on 2026-10-13:

- the lift-angle spreadsheet linked from the Watch Repair Talk forum
  (https://docs.google.com/spreadsheets/d/teAKORqEYkv8yFz96BRHbkg),
  with maker, calibre, size, beat rate and lift angle;
- WatchGuy's list (https://watchguy.co.uk/cgi-bin/lift_angles), maker,
  calibre and lift angle;
- Watchmaking Tools' list
  (https://watchmakingtools.com/watchmaking-resources/watch-movement-lift-angles/),
  maker, calibre and lift angle.

None of them states a licence. Each row names the list or lists it came
from and the row as the list wrote it. The figures are as published and
were not checked against the movements; the checked entries above them
keep their own sources.

How the rows were merged:

- One row per maker, calibre, beat rate and lift angle. Where the lists
  give a calibre two beat rates or two lift angles, it gets a row for
  each, named with the figures, e.g. "ETA 2824 (21,600 bph, 53°)",
  because unnamed variants exist and a list can be wrong; the notes say
  what the other rows give. These rows were first named with "vph"; the
  old names are kept in `former_names`, so a saved pick or a take that
  names one still finds the row.
- Where a list agrees with a checked entry, its source is added to that
  entry. Where it disagrees, the checked entry stays as it is and the
  list's figures become a row of their own.
- A balance-wheel period is derived from the beat rate when there is
  one. No other wheel periods come from the lists.
- Left out: rows with neither a lift angle nor a beat rate, rows with
  no calibre name, electronic movements, pin-lever makers (Roskopf,
  Timex), and beat rates under 3,600 or over 72,000 bph. A beat rate
  under 100 bph in the spreadsheet is read as a typing slip and left
  out of the row, with a note. Calibre names the spreadsheet turned into
  dates ("13-Mar") are read back as numbers ("13.3").

## Chinese makers' own pages

Rows with `confidence` "maker's page" come from the makers' own
specification pages, read on 2026-10-13:

- Peacock (Dandong): http://en.cpeacockmovement.com/Movement/2.html to
  /Movement/8.html and each calibre's page;
- Shanghai Jinghe: http://en.jingheshiye.com/jingdianzhizuo/ and its
  other product lists;
- Tianjin Seagull: https://en.seagullwatch.com/ movement pages;
- Hangzhou: two assembly drawings (HZ8230A, HZ8220A) downloaded from
  Hangzhou's site.

Each row cites the page it came from and the beat rate as the page
wrote it. Only Hangzhou's drawings give a lift angle; the other pages
give the beat rate, jewels, size and power reserve, so those rows carry
a beat rate and a balance period and no lift angle. Seagull's pages print
the beat rate as a bare number ("Frequency: 21600"), read as bph. Pages
with no figures were left out, and none of the pages says which calibre
a movement copies.

Still to do: Hangzhou (hzwatch.com), Beijing (beijingwatch.com) and
Guangzhou Five Goats, now Dixmont (dixmont.com.cn), publish a
drawing per calibre with its lift angle, but their sites refuse
automated readers, so only the two Hangzhou drawings above are in.
Shanghai Watch Co. (shwatch.cn) shows no movement pages. These makers'
calibres are wanted in the table.

## What users add

Wheel periods a user enters and calibres they add are not written here:
they live in `calibres.json` in the user's settings folder and are read
over this table (`timegrapher_core::calibre_store`). A user can send them
to the project as a pre-filled GitHub issue; accepted ones are folded
into this table with the issue as their source.

## How the periods were worked out

- **balance**: one full swing is two beats, `7200 / bph` seconds.
- **escape wheel**: a lever escape wheel moves one tooth per swing, so
  `teeth × 7200 / bph`. At 28,800 bph a 20-tooth wheel turns in 5.0 s and
  a 15-tooth wheel in 3.75 s; at 21,600 bph a 15-tooth wheel turns in 5.0 s.
- **fourth wheel** (or **seconds wheel** / **seconds pinion** where the
  seconds are driven indirectly and the fourth wheel's own period is not
  known): 60 s when it carries the seconds hand.
- **centre wheel**: 3600 s where it sits at the centre and carries the
  minute hand. For the photographed 4130, this slot names the second wheel
  even though it is off centre; its 3600 s constraint is supplied by the
  contributor.
  For the photographed 31xx, this slot represents the second wheel at
  **2160 s**, with the one-hour cannon pinion listed separately.
  The ETA 2824 contribution stores its off-centre intermediate wheel at
  46200/13 s and the cannon pinion separately at 3600 s. Where the great
  wheel period remains unknown (Valjoux 7750), only the **cannon pinion**
  is listed at 3600 s.
- **third wheel** and **barrel**: only from tooth counts or a published
  ratio: third = fourth × third-wheel teeth / fourth-pinion leaves, barrel
  = centre × barrel teeth / centre-pinion leaves.

Where both a wheel's teeth and the next pinion's leaves are given, the
unit tests check the two periods agree. A wheel whose period is not known
is left out rather than guessed.

## Microscope photo contributions

[32xx train counts](../../../docs/32xx-train-wheel-counts.md) contain nine
numbered images, six original photographs, and the complete ratio checks.
Peter Hudson contributed the photos and confirmed applicability to all
genuine Rolex, Dandong and Jinghe 32xx on 2026-10-19. The covered rows,
including the separate Rolex 53° settings, carry the counted wheel teeth
and pinion leaves with `confidence` "derived from tooth counts".

Manufacturer-specific aliases are used for clones. The ambiguous
`3235 (clone)` alias was removed; saved selections using it need an explicit
DD/JH/VR/SH designation. The separate VR32xx contribution below supplies
its own train; SH32xx remains unmeasured. ETA 2824 and its explicitly
covered 4 Hz derivatives also have separate evidence below. Future
contributions must state which makers and variants they cover rather than
propagate counts by a shared calibre number alone.

[4130 train and chronograph counts](../../../docs/4130-train-wheel-counts.md)
contain twelve numbered figures and ten originals from a DD4130, contributed
by Peter Hudson on 2026-10-19. He reports matching counts and periods for
genuine Rolex 4130 and SH4130; the latter now has its own entry. The train
is barrel 105, second 90/15, third 96/12, fourth 120/12, escape 20/10.
**The fourth receiving pinion's 12 leaves are inferred**, not directly
counted. The supplied third-wheel identity is consistent with that inference,
not independently proved by it. The derived periods are 25200, 3600, 480,
60 and 5 seconds respectively. The separate 30:30 and 70:70 chronograph
meshes are documented with engagement state rather than added as unconditional
runtime periods. Existing 4131 entries are unchanged; 7750-based homages
are excluded. The ambiguous `4130 (clone)` alias is removed; select an
explicit manufacturer. Rolex, Dandong and SH4130 all specify **52°** lift:
Rolex publishes this value, and Peter separately confirms the same angle
for all 4130 variants. It is not inferred from tooth counts.

[31xx train counts](../../../docs/31xx-train-wheel-counts.md) contain nine
numbered figures and five originals from the VR31xx photo set, contributed
by Peter Hudson on 2026-10-19. He reports applicability to genuine Rolex,
DD/VS and VR31xx, including 3130, 3135, 3186 and 3187; SH31xx is excluded.
The train is barrel 100, second 81/12, third 96/12, fourth 120/18, escape
20/10. The derived periods are 18000, **2160**, 320, 60 and 5 seconds.
The second wheel occupies the established `centre wheel` slot at 36 minutes;
the cannon pinion is a separate one-hour entry. This corrects the prior
one-hour second-wheel assumption. The new Dandong 3135 entry has DD/VS
aliases and leaves lift angle unspecified. Existing Rolex 3131, Rolex
3135 and VR3135 lift settings remain; SH3135 is unchanged.

## How far to trust it

Tooth counts are rarely published. Only the Unitas 6497-2 / 6498-2 has a
full published train (escape 15 on a 10-leaf pinion, fourth 120/8, third
60/10, centre 80). The 31xx, 32xx, 4130 and DD324 photo contributions supply
train counts including the barrels, with applicability reported by the contributor and the hidden
4130 fourth pinion explicitly marked as inferred. Other entries rest on the
beat rate and layout, plus these:

- **20-tooth escape wheels at 28,800 bph.** The ETA 2824-2's escape wheel
  is documented with 20 teeth. Rolex's own escapement patent (US8529122)
  uses a standard 20-tooth Swiss lever as its baseline (its own design
  has 24 teeth), SJX reports that Chronergy kept the old tooth count, and every 28,800 bph watch recorded in this
  project (Rolex 3235, Dandong 3235 and 4130, Patek 324 clone, Peacock
  SL1258) shows a rate cycle of exactly 5.00 s locked to 40 beats. The 32xx
  photo contribution now directly counts 20 teeth, with its stated coverage
  of Rolex, Dandong and Jinghe. The 4130 contribution also counts 20 teeth;
  Rolex's own 4130 manual
  independently specifies 20. The 31xx photographs also count 20 teeth.
  Other Rolex calibres retain their earlier
  inferred counts and recorded clones their measured basis (the Dandong
  3235 recorded here carries a genuine escape wheel).
  The 2892-A2 and 7750 still have no escape count. The Seiko/Miyota
  research contribution below supplies illustrated counts for its explicitly
  covered variants, with the manufacturer-part mappings recorded separately.
- **Barrels.** Rolex, DD/VS and VR31xx: 5 h from the photographed 100:12
  barrel-to-second mesh and 2160 s second wheel, agreeing with the earlier
  Rolex 3135 value from SJX's 5:1 barrel to minute ratio.
  Rolex, Dandong and Jinghe 32xx: 25,200 s (7 h), from the photographed
  98-tooth barrel and 14-leaf second pinion. This supersedes the provisional
  Rolex 3235 period of 22,050 s from a reading of SJX's 98:16 ratio.
  Rolex, Dandong and SH4130: 7 h from the photographed 105:15 ratio and
  contributor applicability; Rolex's 4130 technical manual independently
  publishes the seven-hour drum period. This is distinct from power reserve.
- **Clones.** The 31xx contribution covers DD/VS and VR, excluding SH31xx.
  The 32xx contribution explicitly covers Dandong and Jinghe
  and excludes VR32xx and SH32xx. The 4130 contribution covers DD4130 and
  SH4130; Peter separately confirms that both share the genuine 52° lift
  angle. Matching counts do not establish parts interchangeability.
  For the older sources, a clone is given
  the original's train, escape wheel
  included, only where it is described as a true clone that takes genuine
  parts (DD3235 and DD3285, JH3235, VR3135, DD4130), or whose maker says
  nearly all parts interchange (DD4131). Design clones and hybrids
  (unqualified SH3135, SH3285, SH4131) and 7750-based "4130" clones
  get only what follows from the beat rate and layout. VR32xx now has
  separate photographic evidence; explicitly ETA-based SH31xx homages
  use a qualified entry. Most older clone facts
  come from two guides by one author on a watch forum (one on clone
  movements, one on lift angles) and teardown threads, cited by URL per
  entry; the escape wheel of a clone resting on them alone is marked
  inferred. Each clone entry's notes say where its lift angle, and
  whether it is a true clone, a part clone or a hybrid, rest on those
  guides alone or on one other source alone. Other remarks in clone
  notes (power reserves, factory prefixes, related calibres) mostly
  rest on a single source, often those guides, and most are not marked.
- **Lift angles** are what sources give. The Rolex 4130 technical manual
  publishes 52°, as does the Rolex 3135 technical information. For the other
  Rolex, AP and Patek entries the figures here come from secondary sources or
  bench settings (Rolex 32xx and 4131 55° with a reported Rolex
  revision of the
  32xx to 53°, AP 3120 53°, AP 4302 52°, Patek 324 52°). The SH4131
  has a T-shaped pallet fork, not the 4131's; it is set at 52°, as the
  31xx, whose fork geometry it resembles (a bench setting, not a
  published figure). The Miyota 8215 is 49°, not the 52°
  timegrapher default.

## Not found

Third-wheel and barrel tooth counts for every calibre except as above;
escape wheel counts for the 2892-A2, 7750, Powermatic 80, AP and the
Seiko/Miyota variants outside the documented research coverage; the AP clones' trains; manufacturer-published train data for
Dandong 324 or Peacock SL1258 (photographed counts are documented below); lift angles for the Sellita, Powermatic 80,
Seagull and Patek 324 calibres. Corrections with a source are welcome.

Built from manufacturer documents, technical guides, parts catalogues,
watchmaking references and forum teardowns, cited per entry; nothing was
taken from any other timegrapher program.

## ETA 2824 microscope contribution

See [the evidence and coverage audit](../../../docs/2824-train-wheel-counts.md)
for the 28,800 bph counts, exact fractional second/barrel periods, cannon
mesh inference, and source-labelled transfers to 39 catalogue rows.
Three-hertz, Power+ and ambiguous clone identities are excluded.

## VR32xx microscope contribution

[VR32xx evidence](../../../docs/vr32xx-train-wheel-counts.md) establishes the
100 / 80:20 / 99:12 / 120:11 train and five-hour barrel. The 20:10 escape pair
is the contributor-accepted standard specification, supported by both
comparison families. Its source note records that it was not photographed. VR and SH aliases are separated.

## Seiko and Miyota manufacturer-document research

[Research and limitations](../../../docs/seiko-miyota-train-research.md)
records illustrated escape counts and exact part-identity transfers for
42 catalogue rows. Fifteen escape teeth at 21,600 bph and twenty at
28,800 bph both give five seconds. The source manifest audits all 182
pre-existing Seiko/Miyota rows and distinguishes unresolved entries.

These are counts from manufacturer drawings, not microscope measurements.
Third-wheel, barrel and pinion counts remain unestablished. The Miyota
9015 family's one-hour entry now names the cannon pinion, not its off-centre
centre wheel. Indirect seconds do not by themselves establish the fourth
wheel period. Broad 6R/7S aliases are removed, and Seiko revision-specific
lift angles are retained.

## DD324 and Peacock SL1258 microscope contribution

[DD324 evidence and ratio checks](../../../docs/dd324-train-wheel-counts.md)
contains eight unchanged originals and numbered count figures contributed
by Peter Hudson on 2026-10-21. The complete photographed train is barrel 93,
second 60/18, third 90 with 10 leaves on each face, fourth 84/9 and escape
20/7. Periods are 18600, 3600, 600, 60 and 5 seconds respectively.
The second wheel uses the established `centre wheel` slot.

The separate nine-leaf centre-seconds pinion is driven by the third-wheel
rim. The 60-tooth cannon wheel is driven by a ten-leaf third-wheel pinion;
its own 28-leaf pinion serves the motion works. Their independently supplied
60-second and 3600-second periods both establish a 600-second third wheel.
The fourth wheel was subsequently photographed, so its 84/9 counts and
60-second period are now established independently of the seconds hand.

Peacock SL1258 receives a contributor-confirmed family transfer from the
DD324 specimen. Genuine Patek 324 entries and all other rows are unchanged.
DD324 retains the existing 52° lift setting; SL1258 remains unspecified.

## Patek 240 clone photographs

The [Patek 240 Clone count record](../../../docs/pp240-clone-train-wheel-counts.md)
preserves six original photographs and ten numbered figures. All six rotation
periods derive from the counted train and the contributor-confirmed 21,600 vph
beat rate. The second wheel has 15 leaves on its near section and 16 on its far
section. The confirmed 16-leaf barrel input gives a 6-hour barrel period; the
15-leaf section serves the dial-side motion works.
Manufacturer and lift angle remain unconfirmed.
