# What every code means, and what to do about it

Every check the program makes comes back with a stable `code`, a
`severity`, the `evidence` (the numbers that triggered it) and `advice`.
This page says, for each code, whether it is about the watch or about the
measurement, how much weight to give it, what to do next and how to put
it to the person. The thresholds are the program's defaults; a session
file can change them under `[limits]` (see the repository's
docs/sessions.md).

Severity: `fault` spoils the reading or is something a watchmaker must
see; `warning` (the session docs call it "check") is outside tolerance
or worth a second look; `note` is information.

## Read them in this order

1. **Is a watch heard, and is the measurement sound?** `watch_heard`,
   the grades in `confidence`, the `doctor` codes, `measurement_short`, `unlock_unreliable`,
   clipping, a `series` verdict of `measurement`. If any of these
   applies, fix the set-up and measure again before reading anything
   else. Say so plainly: *"I can't judge the watch from this; let's get
   a cleaner recording first."*
2. **Faults**: `amplitude_very_low`, `beat_error_large`.
3. **Warnings**, then **notes**.
4. **Never judge on one code.** A verdict weighs the findings with the
   calibre (what is normal for it), the time since the last service and
   how hard it's worn: see
   [when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md).

## Before anything: `watch_heard`

`analyze`, `record` and each `session` reading carry `watch_heard`. When it is false,
the beats are no steadier than noise: an empty stand, a stopped watch,
or the watch not touching the microphone. Every other number in the
result describes noise. Don't show them; tell the person no watch was
heard and check it's running and clamped.

## What limits the readings: `caveats`

`analyze` and each `session` reading's `measurement` carry `caveats[]`:
what limits how far the readings can be trusted, each with a `code`.
Empty means nothing does. Read it before quoting any number, and say
the ones that apply in plain words.

| Code | Means | Do | Tell them |
| --- | --- | --- | --- |
| `no_watch` | No watch heard; the readings are withheld. | As for `watch_heard` false, above. | "I couldn't hear the watch." |
| `set_rate_disagrees` | The beat rate given (`--bph` or the calibre's) is not the one the recording runs at; `heard` is the one it does. Every grade is poor: quote no reading, not even to explain it. | Check the calibre; run `analyze` again without `--bph`. | "The watch isn't beating at the rate I was told; let me check which movement it is." |
| `short_span` | Under 10 s of beats (`span_s`). Every grade is poor: quote no reading. | Record longer: `record` takes 2 min a position. | "That was too short to judge; let's record it longer." |
| `clipped` | A share of beats clipped (`fraction`). Amplitude and beat error one grade down; the rate is not affected. | Lower the input level (`doctor`) and record again. | "The microphone was too loud, so the amplitude may be off; the rate is fine." |
| `beats_missing` | Only a share of the expected beats found (`fraction`). Every grade one step down. | The level, the clamp, then the beat rate (as above). | "I missed some beats; let's get a cleaner recording." |
| `lift_unknown` | No calibre or lift angle given, so the amplitude rests on the default. Amplitude one grade down. | Ask what the watch is and pass `--calibre` or `--lift`. | "The amplitude depends on the movement; tell me what it is and I'll check it." |
| `clock_unknown` | The sound card's clock error isn't known. Rate one grade down. | For a rate within a few s/d, see "The rate and the card's clock" below. | "The rate includes the sound card's own clock error, which this recording can't measure." |
| `breaks` | The take was paused or resumed (`count`). Rate and beat error are fitted on each side of a break apart. | Nothing to fix; say the take was paused. | Only if asked. |

`session` takes the lift angle as known, so its readings never carry
`lift_unknown`.

## How sure is each reading?

`analyze` gives each headline reading a ± and a grade in `confidence`:
`rate`, `amplitude` and `beat_error`, each with `se` (in the reading's
own unit) and `grade`.

| Grade | Rate | Amplitude | Beat error |
| --- | --- | --- | --- |
| `good` | within ±0.5 s/d | within ±2° | within ±0.05 ms |
| `fair` | within ±2 s/d | within ±5° | within ±0.1 ms |
| `poor` | wider | wider | wider |

`doubts` lists what lowered a grade by one step: `clipped`,
`beats_missing`, `lift_unknown` (no calibre or lift angle given, so the
amplitude rests on the 52° default) and `clock_unknown` (the sound
card's clock error isn't known). `confidence` is null when no watch is
heard.

How to use it:

- **Quote the ± with the number** when it matters to the verdict:
  "about 250°, give or take 2°" is honest; "250°" from a `poor` reading
  is not.
- **A rate graded `poor` only because of `clock_unknown`** is still usable
  when what you conclude from it is far larger than the sound card's own
  clock error (1–4 s/d, as `timegrapher --help` puts it): a mean of
  +25 s/d or a spread of 80 s/d between positions is not the card. Say the
  card's clock is unmeasured and that it shifts every position alike.
- **Don't judge on a `poor` reading** otherwise. Fix what `caveats` names (the
  level, the clamp, the lift angle) and measure again. A `poor`
  amplitude next to a `good` rate is common on a weak signal; the rate
  can still be used.
- **The ± is precision, not accuracy.** A wrong lift angle moves the
  amplitude by far more than its ±, and a sound card's clock moves every
  rate by the same amount. `lift_unknown` and `clock_unknown` are
  there to remind you.
- **The rate and the card's clock.** `overall.rate_s_per_day` is on the
  sound card's clock, as other timegraphers read it. When the recording
  carries the USB microphone's 1 kHz lines, `usb_clock` gives the card's
  error (`ppm` slow, `lines`, `spread_ppm`) and `rate_corrected_s_per_day`
  the rate with it removed. A card is often tens of ppm out (1 ppm is
  0.09 s/d), so for a rate within a few s/d, quote the corrected rate
  and say so.

## `doctor`

| Code | Means | Do |
| --- | --- | --- |
| `silent` | Nothing reaches the program. | Wrong input, muted, or unplugged. Check `devices`, the system's input and the cable. |
| `too_quiet` | The ticks are too faint to time well. | Raise the input level (ask first), press the watch firmly into the clamp. |
| `clipping` | The level is too high; the tops of the ticks are cut off. | Lower the input level (ask first). Amplitude and beat error from a clipped recording are not to be trusted; the rate is. |
| `hot` | Close to clipping. | Lower the level a step. |
| `agc_suspected` | The computer's automatic gain is changing the level between ticks. | Turn automatic gain off (ask first); levels mean little while it acts. |
| `no_ticks` | No steady beat heard (signal band `none`). | Is the watch running and touching the microphone? |
| `noisy` | Signal band `poor` (fault: only the rate can be used) or `fair` (warning). | A quieter room, a firmer clamp, the stand away from the computer's fan. |

Details and per-OS fixes: [setup-and-microphone.md](setup-and-microphone.md).

## `session`

Findings are in `report.findings[]`; `recording` is an index into
`readings` (null for findings across positions). Each reading's
`report.verdicts[]` entry marks rate, amplitude and beat error `within`,
`outside`, `not_judged` or `unreliable`.

### About the measurement

| Code | Severity | Means | Do | Tell them |
| --- | --- | --- | --- | --- |
| `no_watch_heard` | warning | The beats in that recording are no steadier than noise; it shows no readings. | Is the watch running, and pressed against the microphone? Run `doctor`, then record that position again. | "I couldn't hear the watch in that position; let's check it's still running and clamped." |
| `measurement_short` | note | Under 40 s measured in a position. | Record that position again for 2 minutes. | "That position was too short to judge." |
| `unlock_unreliable` | warning | Under 60% of the windows found the first sound of the beat on both sides; amplitude and beat error for that reading are shown with `?` and not judged. | Check the level and the clamp; record again. If it persists on a clean signal, `profile` shows what the engine marked. | "The quiet first part of the tick didn't come through clearly, so I can trust the rate but not the amplitude in that position." |
| `rate_unsteady` | warning | The 10 s rates in one reading spread over 20 s/d. | First rule out a loose clamp, knocks and the room; record again. If it repeats on a clean recording, it is the watch: a long run with `series` says what kind of unsteadiness. | "The rate jumped around in that position; let's check it isn't the set-up." |

A clipped recording also shows up here (over 1% of beats clipped):
amplitude and beat error may be off, the rate is not.

### About the watch

| Code | Severity | Means | Weigh it with | Do |
| --- | --- | --- | --- | --- |
| `amplitude_high` | warning (docs/sessions.md calls it "check"), never a fault | Very high amplitude: the position's median amplitude over 330° at the take's lift angle (Witschi's knocking limit; the `[limits]` key `amplitude_high`), judged only when the unlock was timed reliably. | The lift angle first (one set too high reads amplitude high), then the microphone (a pickup that misses the first sound of the beat reads amplitude high). Knocking is confirmed only by jumps in the trace. | Check the lift angle and the microphone and measure again. If both check out and the trace jumps or doubles: a watchmaker, soon; real knocking can damage the escapement. Without jumps, say the amplitude reads very high, not that it knocks. |
| `amplitude_very_low` | fault | Under 200°. | The calibre's normal (some run low by design), the position (verticals run lower), the state of wind, the time since service. | Fully wound, in a horizontal position, on a calibre that normally runs well above it: service. Otherwise retest fully wound before concluding. |
| `amplitude_tolerance` | warning | Outside the tolerance class's range for horizontal or vertical positions, fully wound. | The calibre and how far outside. 240° dial up is normal for some calibres and tired for others. | Use it as one input to the verdict, never alone. |
| `beat_error_large` | fault | 2 ms or more. | Nothing much: at this size it's a real adjustment problem and the watch may struggle to restart if it runs down. | A watchmaker can usually correct it without a full service. |
| `beat_error_tolerance` | warning | 0.5 ms or more. | Age and kind of watch: up to about 1 ms is acceptable on an older or budget watch. | Mention it; it's an adjustment, done at the next service. |
| `rate_tolerance` | warning | The mean rate over positions is outside the class's range. | Whether the positions agree with each other (`positional_delta`). If they do, it's only regulation. | Regulation: a quick, cheap job for a watchmaker. Not a sign of wear on its own. |
| `positional_delta` | warning | The rate differs by over 10 s/d between positions. | The maker's figure for the calibre (SKILL.md, "Judge the rate against the maker's own figure"). Which positions differ (see below). | Within the maker's figure: nothing to do. Well beyond it: an adjustment or wear question for a watchmaker. |
| `dvh_rate` | note | Vertical and horizontal mean rates differ by 5 s/d or more. | `positional_delta` and the amplitude in both. | Background for the watchmaker; on its own, nothing to do. |
| `vh_amplitude_drop` | warning | Vertical amplitude more than 50° below horizontal. | The calibre and the time since service. | Points at the balance pivots or their oil: a service question. |
| `cycle_wheel` | warning | A regular change in rate or amplitude matching the turn of a named wheel. | Its size against the scatter, and whether it shows in more than one position. | Offer a long run in that position (`series`, then `long`) to confirm before naming the wheel to a watchmaker. Wheel periods by calibre come from the calibre table. |
| `cycle_wheel_fraction` | note | A change at a whole fraction of a wheel's turn: a short, sharp change once per turn. | As above. | As above. |
| `cycle_other` | note | A regular change that matches no listed wheel. | Whether the calibre is known (without it, few wheels are listed). | Note it; a long run tells whether it's real. |
| `shape_unlock_loud`, `shape_extra_sounds` | note | Unusual beat shape: the first sound as loud as the last, or extra sounds around the beat. | These are not yet checked against watches with confirmed faults. | Mention only as "worth a watchmaker's look if anything else points there"; see [tick-shape.md](tick-shape.md). |

Which positions differ tells you where to look (more in
[diagnosis.md](diagnosis.md#what-the-positions-tell-you)): dial up
against dial down is the balance's end shake, pivots or a hairspring out
of flat; vertical against horizontal is the pivots and their oil; the
verticals against each other is poise or the hairspring's centring.

With the 24-hour half, `session` also gives the isochronism per
position (`rate_change`, `amplitude_change` and `beat_error_change` from
the fullest to the least wound reading) and Im (the largest rate change,
leaving out 12H). There are no finding codes for these; read them
yourself:

- **`amplitude_change`**: a fall of more than about 40–50° over the day
  points at the mainspring, the barrel or friction in the train. Worth
  mentioning; it is usually dealt with at the next service.
- **`rate_change`**: a well-adjusted watch changes a few s/d over the
  day. Tens of s/d, or a change that differs a lot from position to
  position, is an isochronism question for a watchmaker (the hairspring
  and its regulation).
- **`beat_error_change`**: beat error is a time, and the balance moves
  more slowly at lower amplitude, so the same offset reads larger in ms
  as amplitude falls. A small rise at 24 h is expected. A rise well out
  of proportion to the fall in amplitude, or one that takes it past
  about 1 ms on a watch that runs down low, matters: the watch may be
  slow to restart. Either way it's an adjustment, not a fault in itself.

## `series` (long runs)

Each series (rate, amplitude, beat error) gets one `verdict`; findings
carry the same codes.

| Verdict / code | Means | Do |
| --- | --- | --- |
| `steady` | Readings scatter about one level. | Good. Nothing to find in this run. |
| `periodic` | A cycle, named after the wheel it matches. | As `cycle_wheel`: name the wheel, its size and whether it shows in amplitude too. |
| `two_states` | The watch switches between two levels. | If Tick and Tock move together, the watch really runs at two levels: a watchmaker's question. If only one side moves, see `measurement`. |
| `measurement` / `measurement_split` | Two levels, but they come from the program's marking of the beat, not the watch. | Not the watch. Say so; if it confuses the readings, it's worth a `report`. |
| `shifting_mean` | The level steps and stays. | Was the watch touched, the room changed, the stand knocked? If not, a watch that changes its rate in steps wants a look. |
| `drifting` | The level slides slowly. | Expected for amplitude as the spring runs down; for rate, compare with `rate_moves_with_amplitude`. |
| `wandering` | Each reading remembers the last, with no cycle or trend. | Usually the room or the support; re-run before concluding. |
| `too_short` | Too few readings. | Record longer. |
| `rate_moves_with_amplitude` | The rate follows the amplitude (s/d per degree given). | Isochronism: how the rate changes as the spring runs down. A strong link is a regulation and hairspring question for a watchmaker. |

More in [long-runs.md](long-runs.md) and the repository's docs/series.md.

## Faults the program names

`analyze` lists possible escapement faults from the shape of the Tick
and the Tock in `faults[]`: `id`, `label` (the fault in a watchmaker's
words), `side` (`tick`, `tock` or `both`), `strength`, `evidence` (the
measures behind it) and `see` (where it is described). `strength` is
`possible`, or `likely` once a rule has been confirmed on a real fault;
for now every rule is `possible`, because no watch with a confirmed fault
has been through them. The list is empty when nothing is seen, and also
when the shape can't be judged: no watch heard, clipping, or amplitude or
beat error too unsteady to grade fair. Empty is not a clean bill of
health; it means nothing stood out.

| `id` | What the program heard | Weigh it with | Tell them |
| --- | --- | --- | --- |
| `unlock_too_strong` | The first sound (the unlock) as loud as the last (the drop). | The side named, and whether it repeats in other positions. | "On the Tock, the start of the beat is louder than it should be; worth a watchmaker checking how the escapement is set." |
| `no_unlock_sound` | The first sound missing on that side, or only two sounds where there should be three. In Witschi's list, an escape tooth landing straight on the impulse face of the pallet stone. | A weak or noisy signal hides quiet sounds: check the grades are good and `doctor` says `good` before believing it. | "I can't hear the first part of the beat on the Tick; on a clean signal that points at the escapement." |
| `escapement_friction` | The second sound merges into the first: extra friction in the escapement. | Low amplitude and years since the last service point the same way: old or dried oil is the usual reason, but only opening it tells. | "It sounds like there's friction in the escapement, which is usually a sign it wants cleaning and oiling." |
| `unlock_uneven` | One side's first-to-second interval much longer than the other's: the lock deeper on one pallet stone than the other, or an extra sound just before the unlock (the impulse pin touching the fork horn) taken for it. The label names both, and the program can't tell them apart. | Beat error and the side named. | "The two halves of the beat differ more than they should; a watchmaker can tell which of two causes it is." |
| `rubbing_after_drop` | Sound lingering after the drop on one side, well above the other: in Witschi's list, the balance or hairspring grazing. | The holder and the case can ring; re-clamp and test again before believing it. | "Something may be rubbing on the Tick side; if it shows again after re-clamping, it's worth a look." |

How to report them:

- **Always "possible"**, in the program's words and yours: a finding is
  worth checking on the bench, not a diagnosis. Never name a broken part
  from it.
- **Check it repeats.** A fault seen in one position and not another, or
  not again after re-clamping, is more likely the set-up. One that shows
  in every position, on the same side, is worth passing on.
- **Put it with the rest.** A possible fault on a watch with good
  amplitude, small beat error and steady rate is a note for the next
  service. With low amplitude or `escapement_friction` beside it, it
  supports a service-soon verdict.
- **Give the owner the words to pass on**: the `label`, the side (Tick or
  Tock) and the position, so the watchmaker knows where to look. `see`
  points at the description in the repository's docs/fault-signatures.md.

## When the tool looks wrong

`measurement`, `unlock_unreliable` that survives a clean set-up, or
readings that disagree with another timegrapher at the same moment: see
SKILL.md, "When the timegrapher itself looks wrong".
