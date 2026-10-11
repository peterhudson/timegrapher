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
   the `doctor` codes, `measurement_short`, `unlock_unreliable`,
   clipping, a `series` verdict of `measurement`. If any of these
   applies, fix the set-up and measure again before reading anything
   else. Say so plainly: *"I can't judge the watch from this; let's get
   a cleaner recording first."*
2. **Faults**: `overbanking`, `amplitude_very_low`, `beat_error_large`.
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
| `overbanking` | fault | Amplitude over 330°. | The lift angle first (one set too high inflates amplitude), then the microphone (a missed first sound reads as huge amplitude). | If both check out: a watchmaker, soon. Real knocking can damage the escapement. Don't wear it hard meanwhile. |
| `amplitude_very_low` | fault | Under 200°. | The calibre's normal (some run low by design), the position (verticals run lower), the state of wind, the time since service. | Fully wound, in a horizontal position, on a calibre that normally runs well above it: service. Otherwise retest fully wound before concluding. |
| `amplitude_tolerance` | warning | Outside the tolerance class's range for horizontal or vertical positions, fully wound. | The calibre and how far outside. 240° dial up is normal for some calibres and tired for others. | Use it as one input to the verdict, never alone. |
| `beat_error_large` | fault | 2 ms or more. | Nothing much: at this size it's a real adjustment problem and the watch may struggle to restart if it runs down. | A watchmaker can usually correct it without a full service. |
| `beat_error_tolerance` | warning | 0.5 ms or more. | Age and kind of watch: up to about 1 ms is acceptable on an older or budget watch. | Mention it; it's an adjustment, done at the next service. |
| `rate_tolerance` | warning | The mean rate over positions is outside the class's range. | Whether the positions agree with each other (`positional_delta`). If they do, it's only regulation. | Regulation: a quick, cheap job for a watchmaker. Not a sign of wear on its own. |
| `positional_delta` | warning | The rate differs by over 10 s/d between positions. | The kind of movement: 30 s/d is normal on a budget calibre, not on a chronometer. Which positions differ (see below). | On a good movement: an adjustment or wear question for a watchmaker. On a budget one: usually normal. |
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

Space for the faults the program will name from the shape of the Tick
and the Tock, each said as likely or possible, never certain. No entries
yet.

## When the tool looks wrong

`measurement`, `unlock_unreliable` that survives a clean set-up, or
readings that disagree with another timegrapher at the same moment: see
SKILL.md, "When the timegrapher itself looks wrong".
