# Reading the program's output: the facts, and what to make of them

The program states what it measured: readings with their ±, how much of
the recording it could use, and what it detected in the sound (clipping,
breaks, no regular beat, a repeating change). It does not judge the
watch. Judging is your job, and this page gives the rules to do it with.
Each rule carries its source: a maker, a certifier or Witschi where one
publishes the figure, or **the project's rule of thumb**, with the
evidence it rests on. Say which when a rule decides your answer.

Builds before schema version 2 also printed grade words (`good`,
`fair`, `poor`), a `verdict`, findings with a `severity` and `advice`,
and named faults in `faults[]`, from the same thresholds as the rules
below. If you meet them, read the numbers behind them with this page,
and never repeat a grade word or a fault name to the person as if it
were a verdict. docs/agent-interface.md's row for each command lists
the fields of the current version.

## Read the output in this order

1. **Was a regular beat found?** If not, nothing else in the result
   describes the watch.
2. **How good is the measurement?** The ±, the signal, clipping, the
   beats found, the span, breaks, the lift angle and the clock
   ([below](#how-good-is-the-measurement)). Fix the set-up and measure
   again before reading a watch from a doubtful recording.
3. **The readings against a tolerance set** chosen for this watch: the
   maker's own figures where they exist, otherwise one of Witschi's
   classes, and say which
   ([when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md#tolerance-sets-by-source)).
4. **Across positions** (`session`), then **over time** (`series`,
   `long`), then **the shape of the beat** (`shape`, `profile`).
5. **Never judge on one figure.** A verdict weighs the readings with the
   calibre, the time since the last service and how hard it's worn:
   [when-to-see-a-watchmaker.md](when-to-see-a-watchmaker.md).

## Was a regular beat found?

`analyze`, `record` and each `session` reading carry `watch_heard`. It
is false when the beats are no steadier than noise: their timing scatter
is 2% of the beat period or more. The program's own evidence for that
line, from the project's recordings: there, a watch's beats sat within
a millisecond of a straight line (at most 0.7 ms, 0.6% of the beat, in
every stored take), while on an empty stand the tracker still found a
"beat" in each window, anywhere in it, and the scatter was about 5% of
the beat (8.1 and 8.6 ms in the silence take at 21,600 bph). On those
recordings 2% leaves a factor of three either side.

`session` states it for the position as a note, `no_regular_beat`:
"No regular beat found (timing scatter X ms, Y% of a beat)", with no
readings beside it.

When no regular beat is found, every reading describes noise. The program
withholds them; don't work any out. `bph` and `beats_found` may still be
there: the beat rate set or listened for, and a count of what the tracker
found in the noise. Neither says a watch was heard.

What to do: ask whether the watch is running, and whether it is clamped
firmly against the microphone; then run `doctor`. When `doctor`'s levels
are clean as well (no clipping, no automatic gain, a sensible peak), say
it plainly: the stand is empty or the watch has stopped. Tell them: *"I
couldn't hear the watch; is it running, and pressed against the
microphone?"*

## How good is the measurement?

Every threshold in this table is the project's rule of thumb unless a
source is named. Use them to decide whether a reading can carry a
judgement, and say what limits it.

| Fact | Where | Rule of thumb, and its evidence | What to do |
| --- | --- | --- | --- |
| The ± of each reading | `confidence.rate.se`, `.amplitude.se`, `.beat_error.se` (one standard error from the spread between 2 s windows, in the reading's unit) | Tight: within 0.5 s/d, 2° or 0.05 ms. Usable: within 2 s/d, 5° or 0.1 ms. Wider: don't judge on it. The tight band sits a little above what the project's regression check counts as a real change (0.3 s/d, 1.5°, 0.03 ms; docs/regress.md). | Quote the ± with the figure ("about 250°, give or take 2°"). A wide ± on amplitude next to a tight one on rate is common on a weak signal: the rate can still be used. |
| The signal | `doctor`'s and the app's `signal_x`: how many times louder the typical beat is than the background between beats | 10× and up: every reading. 5 to 10×: the rate; amplitude and beat error may be off on some watches. 3 to 5×: the rate only. Under 3×: no watch (pure noise reads about 2×). The project's rule of thumb, from its own added-noise tests of 2026-10-09 (not yet published). | Below 10×: press the watch more firmly against the pickup, check the cable, raise the level a little (ask first). The signal measures loudness, not the shape of the tick: a holder that smears the sound can read 10× and still mislead (`profile` shows it). |
| Clipping | `clipping.beat_fraction` (`analyze`), the `clipped` caveat's `fraction` | Over 1% of beats with a clipped sample: amplitude and beat error may be off; the rate is not (`session` lists it as a `clipping` finding; docs/sessions.md). | Hold neither amplitude nor beat error to a tolerance. Lower the input level (`doctor`, ask first) and record again. |
| Beats found | `beats_found` against duration × bph ÷ 3600; the `beats_missing` caveat | Under 95% of the beats expected: every reading is less sure. | First the beat rate (run `analyze` again without `--bph`), then the level and the clamp. A forced wrong beat rate finds a fraction of the beats and gives a meaningless amplitude. |
| Span | `short_span` caveat (`span_s`); a `session` reading's measured time | Under 10 s of beats: quote no reading. In a session, the program lists under 40 s measured in a position as `measurement_short`, after Witschi's testing methods D40e ("≥40 s measurement time", about p. 34). For comparison, Witschi's training course (p. 53) sets about 15 s to stabilise and at least 20 s to measure per position; Sellita's SW200-1 sheet 20 s and 40 s; Seiko's NH35 and 6R35 guides at least 20 s each. | Record that position again: `record` takes 2 minutes. |
| Breaks | `breaks` (`count`; `audio_s`, `gap_s` on long runs) | A fact, not a fault: rate and beat error are fitted on each side of a break apart. Where the beat error's sign turns over across a break (or, in a `session` reading, across a masked stretch), both beat errors are given as positive sizes, and `long`'s slices and `series`' beat error readings are then sizes too; `analyze`, `session`, `long` and `series` all say so with the caveat `beat_error_as_size`. When `long`, `series` or `session` read a take's clock log (`long` and `series` read one only with `--clock`), a forward step of 0.5 s or more in it (the computer's time jumping ahead of the audio and staying ahead, not one late entry) that no pause listed in `session.json` explains counts as a break of its own, so no reading is measured across it. | Say the take was paused, if it matters. |
| Timing scatter | `jitter_us` against the beat period | 2% of the beat or more: no regular beat (above). Below that, compare it between takes of the same watch: a jump with nothing else changed points at the set-up. | |
| Lift angle | `lift_deg`; the `lift_unknown` caveat | No calibre or lift angle given: the amplitude rests on the 52° default. | Find the calibre's lift angle before quoting amplitude: ask what the watch is; pass `--calibre` or `--lift` ([reading-the-numbers.md](reading-the-numbers.md#lift-angle)). |
| Clock | `clock.from`, `usb_clock`, `rate_corrected_s_per_day`; the `clock_unknown` caveat | A sound card is off by 10–50 ppm, 1–4 s/d (`timegrapher --help`); 1 ppm is 0.09 s/d. | See [the rate and the card's clock](#the-rate-and-the-cards-clock). |
| The unlock found on both sides | a `session` reading's unlock coverage | Under 60% of the 2 s amplitude windows (the project's rule of thumb): amplitude and beat error from the unlock are doubtful; the rate is not. | Check the level and the clamp, try the watch cased or on another stand; `profile` shows what was marked. |
| Set beat rate against heard | the `set_rate_disagrees` caveat (`heard`), `bph_heard` | The recording runs at another beat rate than the one set. | Quote no reading, not even to explain it. Check the calibre; run `analyze` without `--bph`. |

Two points that apply to all of them:

- **The ± is precision, not accuracy.** It says how much the reading
  would move on another stretch of the same recording. A wrong lift angle
  moves the amplitude by far more, and the card's clock moves every rate
  alike; neither shows in the ±.
- **A rate limited only by the card's clock** is still usable when what
  you conclude from it is far larger than 1–4 s/d: a mean of +25 s/d or a
  spread of 80 s/d between positions is not the card. Say the clock is
  unmeasured and that it shifts every position alike.

## What limits the readings: `caveats`

`analyze` and each `session` reading's `measurement` carry `caveats[]`,
each with a `code`; `long` and `series` carry one too, with only
`sides_by_count` and `beat_error_as_size`. They are facts about the recording. Empty means none
applies. Read it before quoting any number, and say the ones that apply
in plain words.

| Code | Means | Do | Tell them |
| --- | --- | --- | --- |
| `no_watch` | No regular beat found; the readings are withheld. | As [above](#was-a-regular-beat-found). | "I couldn't hear the watch." |
| `set_rate_disagrees` | The beat rate given (`--bph` or the calibre's) is not the one the recording runs at; `heard` is the one it does. | Quote no reading. Check the calibre; run `analyze` again without `--bph`. | "The watch isn't beating at the rate I was told; let me check which movement it is." |
| `short_span` | Under 10 s of beats (`span_s`). | Quote no reading. Record longer. | "That was too short to judge; let's record it longer." |
| `clipped` | A share of beats clipped (`fraction`). | Over 1%: hold neither amplitude nor beat error to a tolerance; lower the level and record again. | "The microphone was too loud, so the amplitude may be off; the rate is fine." |
| `beats_missing` | Under 95% of the expected beats found (`fraction`). | The beat rate, then the level and the clamp. | "I missed some beats; let's get a cleaner recording." |
| `lift_unknown` | No calibre or lift angle given; the amplitude rests on the default. | Ask what the watch is. | "The amplitude depends on the movement; tell me what it is and I'll check it." |
| `clock_unknown` | The sound card's clock error isn't known. | For a rate within a few s/d, see below. | "The rate includes the sound card's own clock error, which this recording can't measure." |
| `breaks` | The take was paused or resumed (`count`). | Nothing to fix. | Only if asked. |
| `sides_by_count` | At `count` of those breaks Tick and Tock were not told apart by their sound, so the sides after each are named by the beat count across it. | Nothing to fix. | Only if asked. |
| `masked` | `session` readings only: `seconds` of the measured stretch were masked out of the readings by hand (a session file's `masked`, or the app's Paper Strip). | Nothing to fix; say the readings leave that time out. | "Some of the take was masked out of the readings by hand." |
| `beat_error_as_size` | The beat error's sign differs either side of a cut in the stretch, so both beat errors are given as sizes, positive, not signed; in `long` the slices' beat errors too, and in `series` the beat error readings, their median and the tests on them. | Name no side from the sign; quote the size. | Only if asked. |

`session` takes the lift angle as known, so its readings never carry
`lift_unknown`.

## The rate and the card's clock

`overall.rate_s_per_day` is on the sound card's clock, as other
timegraphers read it. When the recording carries the USB microphone's
1 kHz lines, `usb_clock` gives the card's error (`ppm` slow, `lines`,
`spread_ppm`) and `rate_corrected_s_per_day` the rate with it removed.
For a rate within a few s/d, quote the corrected rate and say so. Name
the clock with every rate you quote ("on the sound card's clock" or
"corrected for the card's clock"). When comparing with another
timegrapher, a reference reading or a buyer's machine, lead with
`overall.rate_s_per_day`, since that is how they read it, and give the
corrected rate beside it.

## `doctor`

`doctor` reports levels and lists what it detected, each with a `code`
and its `evidence`. Its thresholds are the project's, with the reasons
the program gives for each:

| Code | Detected | Rule of thumb | Do |
| --- | --- | --- | --- |
| `silent` | No signal at all. | Peak below about −80 dBFS. | Wrong input, muted, or unplugged: check `devices`, the system's input and the cable. On macOS, the terminal may lack microphone permission. |
| `clipping` | Samples at full scale. | 3 or more clipped samples. Clipping flattens the ticks and spoils amplitude and tick shape. | Turn the input level down (and automatic gain off); it suggests about −6 dB and a re-check. Ask first. |
| `hot` | Peaks close to full scale. | Peak above −6 dBFS: tick loudness varies by a few dB through a run and between positions, and a fully wound watch at high amplitude is louder still. | Turn the input down a little (ask first). |
| `agc_suspected` | The background rises between ticks. | A rise of over 3 dB through the gap: automatic gain turning down on each tick and back up in the gaps. | Turn the input's automatic gain off, so every tick is measured at the same gain (ask first). Levels mean little while it acts. |
| `no_ticks` | Few beats. | Fewer than half the expected beats, or a signal under 3×. | Is the watch running and clamped firmly against the microphone? Is this the right input? Is the beat rate right (give `--bph`)? |
| `too_quiet` | A quiet input. | Peak below −30 dBFS. | Raise the input level so ticks peak near −10 dBFS, the level the program aims for: well above the card's own noise, with headroom for a louder watch (ask first). |

`suggested_gain_change_db` says how far to move the level. Details and
per-OS fixes: [setup-and-microphone.md](setup-and-microphone.md).

## `session`: the Positions Test

`session` gives each position's readings and, for each state of wind,
the characteristic values docs/sessions.md defines:

- **X**, XH, XV: the mean rate over all, the horizontal and the vertical
  positions.
- **D** (`d_rate`, `d_amplitude`): the largest difference between any
  two positions. DV and DH: the same among the vertical or the
  horizontal positions only.
- **DVH** (`dvh_rate`, `dvh_amplitude`): the vertical mean minus the
  horizontal mean. Negative amplitude means the verticals read lower.
  For "vertical against horizontal", quote `dvh_amplitude`, never
  `d_amplitude`.
- **Di**: 6H minus CH.
- With the 24-hour half: each position's change (`rate_change`,
  `amplitude_change`, `beat_error_change`), Im (the largest, leaving out
  12H, as NIHS 93-10 does), Ie (the change in X).

Judge them with these rules. "Fully wound" means within about 2 hours of
a full wind (the project's rule of thumb).

| What to look at | Rule, and its source | What it points at, and what to say |
| --- | --- | --- |
| X against a tolerance set | The maker's figure for the calibre where one exists; otherwise a Witschi class ([tolerance sets](when-to-see-a-watchmaker.md#tolerance-sets-by-source)). Say which. | Outside, with the positions agreeing among themselves: regulation, a quick job for a watchmaker, not a sign of wear on its own. Witschi's training course: "adjust first the beat error, and then readjust the rate". |
| D, the spread between positions | Against the maker's own figure where published (ETA 2824-2: 30, 20 or 15 s/d by grade; Miyota 8215 under 50 s; Seiko NH35 under 60 s; COSC's largest variation 10 s/d). Otherwise over 10 s/d is the project's rule of thumb for "worth a look". | Within the maker's figure: nothing to do. Well beyond it: an adjustment for a watchmaker. Which positions differ says where: below. |
| DVH rate | 5 s/d or more either way is the project's rule of thumb for mentioning it. | Background for a watchmaker. For small rate differences between horizontal and vertical, Witschi's training course: "adjust distance of regulator pins, for V − close pins and for V + open pins". A free-sprung balance has no pins. |
| DV, the spread among the vertical positions | No threshold. | Large rate differences between the vertical positions: Witschi's training course gives "centering, balancing or replacing of the complete regulating organ" (the hairspring's centring, the balance's poise). |
| Vertical amplitude against horizontal (`dvh_amplitude`) | Witschi's acceptable ranges give vertical its own, lower range ([tolerance sets](when-to-see-a-watchmaker.md#tolerance-sets-by-source)). More than 50° below horizontal is the project's rule of thumb. | Some loss is normal (pivot friction). A large one points at worn or dry balance pivots or jewels, or a bent pivot: a service question. |
| Amplitude low | Below the maker's floor where one exists (ETA 2824-2 and Sellita SW200-1: 200° at 6H after 24 h). Under 200° fully wound, in any position, is the project's rule of thumb for "very low". | Check the lift angle first. Fully wound, horizontal, on a calibre that normally runs well above it: usually a movement that wants servicing (dirty or dry oil, a weak mainspring, power lost in the train). Otherwise retest fully wound before concluding. |
| Amplitude high | Over 330°, judged on the position's median at the take's lift angle: the top of Witschi's acceptable horizontal range, and the amplitude Witschi's training course (p. 15) names as the cause of knocking. ETA's and Sellita's sheets put the maximum at 315° dial up fully wound. | Check the lift angle (one set too high reads high) and the microphone (a pickup that misses the first sound of the beat reads high). Real knocking shows as jumps or doubles in the trace, not as a number: say the amplitude reads very high, not that it knocks. If the trace jumps: a watchmaker, soon. |
| Amplitude against a set's range | Witschi's classes give horizontal and vertical ranges, fully wound ([tolerance sets](when-to-see-a-watchmaker.md#tolerance-sets-by-source)). | One input to the verdict, never alone: 240° dial up is normal for some calibres and tired for others. |
| Beat error | Witschi: 0.0–0.5 ms acceptable. ETA 2824-2: 0.8 ms (Standard, Elaboré) or 0.6 ms (Top) dial up fully wound. 2 ms or more is the project's rule of thumb for "large". | An adjustment, not wear. At 2 ms or more the watch may be slow to restart if it runs down; a watchmaker can usually put it in beat without a full service, and before regulating (Witschi, above). |
| Rate unsteady within a position | The 10 s rates spread (5th to 95th percentile) over 20 s/d: the project's rule of thumb. | First a loose clamp, knocks, the room; record again. If it repeats on a clean recording, a long run with `series` says what kind. Witschi's training course: an irregular rate usually comes with insufficient amplitude (an overhaul); large but regular rate variations are "defects in the gear train". |
| A regular change tied to a wheel | Above the 1% false-alarm level and matching a wheel's turn (docs/sessions.md). | Offer a long run in that position (`series`, then `long`) before naming the wheel. Witschi's training course puts large but regular rate variations down to the gear train; the period says which wheel to tell the watchmaker about. A change at a whole fraction of a wheel's turn comes from a short, sharp change once per turn. One tied to no listed wheel may be another wheel or pinion, or a disturbance near the microphone. |
| Quality factor N | Witschi's *Testing methods for mechanical watches* (2025, doc. D40e, about p. 29; the page is approximate) gives N = 0.15·\|Imax\| + 0.1·Pmax + C, with C = 0.6 in its worked example. In `session`'s notation Imax is Im and Pmax is D; the program doesn't compute N. Witschi's 2010 training course doesn't have it. | Don't judge on it: no source here says what N is good. |

Which positions differ tells you where to look (more in
[diagnosis.md](diagnosis.md#what-the-positions-tell-you)): dial up
against dial down is the balance's end shake, pivots or a hairspring out
of flat; vertical against horizontal is the pivots and their oil; the
verticals against each other is poise or the hairspring's centring. A
large D or DVH points at poise or a hairspring out of true.

With the 24-hour half, read the isochronism yourself:

- **`amplitude_change`**: Witschi's training course: "Decrease of the
  values after 24 h: approx −10% to −15% is ok". Much more points at the
  mainspring, the barrel or friction in the train. Worth mentioning; it is
  usually dealt with at the next service.
- **`rate_change`**: against the maker's isochronism figure where one
  exists (ETA 2824-2, dial up from full wind to 24 h: ±20, ±15 or ±10 s/d
  by grade; Seiko 4R35 and 6R15: ±30 s/d). Tens of s/d beyond it, or a
  change that differs a lot between positions, is a question for a
  watchmaker (the hairspring and its regulation).
- **`beat_error_change`**: beat error is a time, and the balance moves
  more slowly at lower amplitude, so the same offset reads larger in ms
  as amplitude falls. A small rise at 24 h is expected. A rise well out
  of proportion to the fall in amplitude, or one past the maker's figure
  on a watch that runs down low, matters: the watch may be slow to
  restart. Either way it is an adjustment, not a fault in itself.

## `series` and `long`: is it steady?

`series` gives, for each of rate, amplitude and beat error, a
`headline` stating what its tests detected, with their statistics, and
the tests themselves. It gives no verdict: the judging is yours, with
the table below. The tests and their limits are documented in
docs/series.md; the 1% level and the others below are the program's
detection limits, and the readings of them are the project's rules of
thumb. `findings[]` (`code`, `series`, `title`, `evidence`) lists one
entry per series where structure was detected.

| Statistic | What it shows | How to read it |
| --- | --- | --- |
| `period` (period, its resolution, the swing peak to peak, the share explained, the false-alarm chance) and `cycle` | A repeating change | A false alarm under 1% that explains a fair share (the program asks 10% of the variance with the slow trend taken out) is a real cycle. Name the wheel it matches; quote the period search's peak to peak as "the swing a timegrapher shows". A period that matches no wheel, or a weak one, is a hint only; if the person asks, one line: the program noted a repeat it can't tie to a fault; ignore it unless a long run confirms it. |
| `changes` (stretches of constant mean) and the CUSUM | A step that stays | A CUSUM inside ±1.36 is one level. A step: was the watch touched, the room changed, the stand knocked, at that time? If not, a watch that changes its rate in steps wants a look. |
| `trend` (straight and curved, the share explained) | A slow slide | Amplitude falls as the mainspring lets down: tens of degrees over a day is expected; a fast fall in a few hours points at a weak or dirty mainspring or barrel. For the rate, compare readings at the same state of wind, and see `rate_amplitude`. |
| `autocorrelation` (Ljung–Box p, the largest short-lag correlation) and `allan` (the excess over the white line, `best_tau_s`) | Readings that remember the last ones | A Ljung–Box p under 0.01 with a correlation of 0.15 or more, or an Allan curve lifting well above the white line: short readings can mislead. Average for longer (`best_tau_s`) before judging. Usually the room or the support; re-run before concluding. |
| `two_state` (levels, `delta_bic`, `separation`, `switches`, `regularity`, `one_sided`, `unlock_jump`), and `unlock_hopping` | Switching between two levels | Two levels are real when ΔBIC is 10 or more, the levels are 2 or more SDs apart, each holds at least 10% of the time, and they switch at least 4 times (the program's limits). If only one side's unlock edges move (`one_sided`), or the unlock beat error jumps three times more than the drop's (`unlock_jump`), it is the unlock mark hopping between two points of the sound (`unlock_hopping` true; the headline says which side's windows fall in two clusters): the measurement, not the watch; read the beat error from the drop and the amplitude with care. If Tick and Tock move together, the watch really runs at two levels, often touching something part of the time: a hairspring coil rubbing, the balance rubbing the cock or the pallet bridge, a hand or the rotor catching. A watchmaker's question, in that position. |
| `rate_amplitude` (slope s/d per degree, `r`, `p_value`, `effective_readings`, and `take_r`, `take_s_per_day_per_deg` for the whole take) | The rate following the amplitude | Always given, with no threshold. A p under 0.01 with \|r\| 0.3 or more is the project's rule of thumb for "the rate moves with the amplitude". On a run-down, the whole-take slope is the movement's isochronism: a strong link is a regulation and hairspring question for a watchmaker. |
| Too few readings | | Record longer. |

More in [long-runs.md](long-runs.md). In `long`'s HTML report, a fault
tied to a wheel shows as a stripe.

## The shape of the beat

The rules for possible escapement faults from the shape of the Tick and
the Tock are in [tick-shape.md](tick-shape.md#rules-for-possible-faults),
read against `shape`'s measures. None has been checked against a watch
with a confirmed fault yet: anything they find is worth checking on the
bench, never a diagnosis.

## When the tool looks wrong

The unlock mark hopping (`two_state` above), the unlock found in under
60% of windows on a clean set-up, or readings that disagree with another
timegrapher at the same moment: see SKILL.md, "When the timegrapher
itself looks wrong".

Two things the project has measured that help here:

- **Tick and Tock amplitudes far apart.** Splits up to about 20° were
  common on the project's clean recordings (docs/validation.md, "Tick
  and Tock amplitudes apart"). A much bigger one usually means one
  side's sounds were misread: `profile` shows where the marks sit.
- **Beat error from the unlock against tg.** Over the corpus the two
  agree to a median of 0.03 ms, but on two of the project's watches,
  whose sides sound different, the unlock figure reads about 0.2 and
  0.57 ms under tg's (docs/validation.md, "Beat error from the unlock
  against tg: two watches apart"). Which is the watch's true beat error
  is not settled, so when another timegrapher disagrees on a watch like
  that, give both and say so.
