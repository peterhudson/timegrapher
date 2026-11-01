# Math Standards Guide

Version 1, approved 2026-10-29. This guide says how every figure is computed: what is averaged, how, over what, what a gap does, how much data a figure needs, and what its ± means. Every PR that changes how a figure is computed is built to it and checked against it. Where today's code differs, the code changes, not the guide, and the issue that changes it is named. A case the guide doesn't cover is raised, and its answer is added here.

Its peers are the [Architecture Guide](architecture.md) (where a rule lives) and the [Design Guide](design/design-guide.md) (how it looks). Audited at main `8b7853e`. Code was read, not run, except where a line says a simulation was run. Labels: N and K are findings of the audit, M of the method audit, R of the red team that reviewed this guide before approval.

---

## Decisions (2026-10-29)

- **Window membership:** a window belongs to the reading whose span holds its midpoint (option B).
- **Percentile:** linear interpolation (Hyndman and Fan type 7, the default in R and NumPy), one core function.
- **Minimum data:** each reading carries "windows used of windows in the span" from core. Below 60% it gets a caveat ("Read from 4 of 15 windows"). With fewer than 3 windows it shows no ±. The figure is still shown, never filled in. This holds until R3 replaces the counts in section 7 with a gate on the ±.
- **Partial windows:** a window short of some beats counts as one window in the median, as long as each side has its minimum of 3 good beats.
- **A listed pause the clock log didn't see:** with a clock log, the log decides. The pause is still a break (beats are renumbered and nothing is fitted across it), but it adds no time, and a caveat says the two disagree. Without a log, the listed gap is added.
- **CUSUM:** scaled by the long-run σ (#647). The scale is s × √(1 + ρ) ÷ (1 − ρ), with s the short-term σ from neighbour differences (for lag-one correlated readings s = σ·√(1 − ρ) and the long-run σ is σ·√((1 + ρ)/(1 − ρ))): 2.45 at ρ = 0.5, 4.4 at ρ = 0.7. ρ is step-robust: the lag-one correlation of the readings after each side's mean is taken off at the CUSUM's largest point, so a real step does not count as correlation, then bias-corrected as ρ̂ + 2(1 + 3ρ̂)/n and clamped to 0 to 0.99. ρ̂ is read only from neighbouring readings that are both present, so a gap never joins readings that were not next to each other, and n is the number of such pairs plus one (the count of readings when none is missing); read across gaps, or with n the readings present, half the readings missing at ρ 0.9 false-alarmed 19% and 10%, against about 5% this way. A wheel's cycle or two-state switching counts as noise: it raises ρ, so the CUSUM flags only a shift larger than the cycle, and the cycle and the switching show in Long runs and Two states. On a seeded test (AR(1), 360 readings, 2,000 runs) false alarms are 4 to 6% from ρ 0 to 0.9, against 85% at ρ 0.5 and 100% at ρ 0.9 today, and steps are caught close to the rate with ρ known. ρ taken from the differences alone false-alarmed 18% at ρ 0.9, and ρ from the raw readings hid real steps. It lands after the autocorrelation fix (#650). A level change is named by one rule, in the headline and on the chart alike: the change finder cut the series, the cut explains at least 20% of its variance, and the lead is not a slide (`steadiness::level_change`). The CUSUM's band is a guide to the eye; a CUSUM past it with no such change names none, and its corner is marked at that change's cut. A small step that only the CUSUM shows goes unnamed, though the line still bends (#697, #717). Whether the 20% share is the right bar is for section 7's rewrite (#721).
- **One-sided windows:** a 2 s window with one side read is left out of the amplitude reading, and kept in that side's Tick or Tock figure. Treating the missing side as equal to the other would be filling.
- **Change finder:** a step is judged against the same step-robust long-run σ as the CUSUM, from the one core function, on the block medians it works on; seeded test: at most 5% false changes on steady readings at ρ 0.9, and a 2 s/d step still found (#649).
- **Autocorrelation and Ljung–Box:** the slow trend is taken off first as the period search takes it off (`longterm::detrend` with `DETREND_S`): a running median over 30 min or a quarter of the take, whichever is shorter, and a quadratic fit on a take under 20 min, so a wind-down does not read as memory while a wheel's cycle (60 s for the fourth wheel) still shows; seeded test both ways (#650).
- **FFT:** the Hann-windowed FFT stays for a take with no gaps. The Hann window tapers the two ends; it does not fill gaps. A take with gaps uses Lomb–Scargle with the same Hann weights: at each period, the sine fitted by weighted least squares to the readings that exist, nothing filled in. On a complete take this is the Hann FFT's size within 0.5%, except at the longest periods, where the fit's 2ω term (the sine's own overlap with the window) moves a bin by a few percent, +4% at 683 s on 900 readings; one reading out leaves the spectrum as it was; an unweighted fit moved the peak of a nearly complete take (683 s to 410 s) and single bins ×0.04 to ×6 (#652, #683).
- **Isochronism:** every 10 s reading is kept (no smoothing over time, so a swing over minutes from a wheel still shows). Each reading's amplitude noise comes from the scatter of its own 2 s windows, and the slope is corrected for it (errors in variables). It is withheld when amplitude changed too little to tell, and carries a ±. The ± is Fuller's (1987, §1.2) variance for a slope with noise in x: residuals from the corrected slope, the true x's variance m̂XX = mxx − σuu (σuu the mean reading noise), and Bartlett's factor on the equation-error term σee/m̂XX only, with ρX = ρx/λ and ρe = ρv·σvv/σee (white reading noise dilutes the lag-one ρ read from the readings; each ρ through `confidence::long_run_rho`, capped at 0.99). With no noise in x it is Bartlett's factor with ρx·ρres. Run: ±1.96 SE holds the true slope 929 to 991 times in 1,000 across ρ 0, 0.5 and 0.9, x noise 0 to 3.5° on a 5° spread, and no gaps, blocks or half missing; Bartlett's factor on all of se/λ held as few as 816 (#651). The cross-correlation slope gets the same correction. Caveats: listed position changes in the take; temperature is not logged. Simulated: true −0.5 s/d per degree reads −0.50 on 2 h of ±5° swings, against −0.40 today (#651). To check on real takes: that noise in neighbouring 2 s windows is independent.
- **From the red team (R1 to R8):** one tiling of 2 s cells on true time, with a 10 s reading as five cells and the ± read from the tiling; Steadiness readings from the core reading function; figures gated by their ± rather than by counts; one core lag-one ρ (`confidence::long_run_rho`), with its bias correction and gap pairing, behind the ±, CUSUM, change finder, isochronism and cross-correlation: `confidence::long_run_sd` turns it into the long-run σ of a mean for the CUSUM and change finder, and a slope's ± uses Bartlett's factor, n_eff from ρx·ρres (with noise in x, on the true x's term only: see Isochronism); with what is taken out before ρ is read named by the caller (a line for the ±, the best single step for the CUSUM and change finder); the clock sign flipped before release so + ppm means the sound card runs fast; beat error says whether it came from the unlock or the drop; one word, cut, with a kind and a width; decimals follow the ±. Filed or noted: R1 on #636, R2 #656, R4 on #647 and #649, R5 #657, R6 #658, R8 on #510. R3 and R7 are this guide's: R7 is in section 2; R3 rewrites section 7 after the four method fixes.
- **Is a watch heard (#675, #678):** over 10 s or more, a timing scatter of 2% of a beat or more (`beats::NO_WATCH_JITTER_FRACTION`) is no watch, whatever the sound. From 0.8% (`beats::NOISE_DOUBT_FRACTION`) up to it, where stand noise can still fall and so can a weak watch about to stop (up to 1.4% in a take's last 25 minutes), and on any stretch under 10 s, the timing says a watch unless the beats' averaged sound shows no unlock or drop on either side; a sound that can't be judged does not veto. Every reader asks `measure::beat_sound_needed` and decides with `measure::no_watch_reason`. Run: stand noise is read as a watch on about 1 in 70 seeded draws of 30 s and 2 in 60 of 90 s; the test names them and the list only shrinks. Decided 30 Oct: a dying watch keeps its readings at that cost.
- **Size of a cycle (#761):** a cycle's peak to peak is read from its folded shape smoothed to its first harmonics: no more than the search summed, no more than the whole cycles in the take, and each one past the fundamental only if it stands out of the AR(1) noise at its own frequency (1% χ² with 2 degrees of freedom, ×π/2 for the fold's medians). On 2.6 cycles a 225 s line read 29.4 s/d with eight harmonics against 10.6 with the rule; on a seeded 60 s sine in ρ 0.8 noise the median swing is 1.06 of true against 1.39 with every harmonic kept. Decided 31 Oct.
- **Whole fractions (#787, #789):** a line is n of a longer period (a wheel's turn, or a found cycle's harmonic) when its frequency is within one resolution (1/span) of n over that period (`longterm::harmonic_offset` under 1). Neighbouring n are one over the period apart, so when the period is longer than half the span two can fit; then no fraction is told. Wheel fractions follow this since #787, and a found cycle's harmonic label since #803. A wheel fraction is marked only when exactly one wheel and n from 2 to 6 fit. On a 580 s span, a 224.84 s line is 0.43 resolutions from half the 540 s third wheel's turn and 0.64 from a third, so it gets none. Decided 31 Oct.
- **Many tests (R9):** after the CUSUM, change finder and autocorrelation fixes, a seeded run of the whole Steadiness pane on a few thousand steady simulated takes (ρ 0 to 0.9, 30 min to 24 h) counts how often any lead shows. Target: at least 95 in 100 steady takes say nothing stands out. If missed, a lead must clear Holm's bar across the pane; each test's own plot and p stay. The run stays as a seeded test.
- **Home:** this file. A chapter for readers of the user docs follows later.

---

## 1. Principles

- **One rule, one definition.** One median, one percentile, one ±, one window rule, all in core.
- **Never invent a value.** A window or bin that reaches into a cut is left out, never filled in (#327). A window short of some beats is made from the beats it has, and counts as one window in a median (section 4).
- **No reading straddles a cut.** Beats either side of a gap, mask, break or clock step are never fitted together.
- **Robust by default.** A reading of many short windows is their median. A rate is a fit over beats. A mean is used only to put the two sides of a window together, to combine stretches by the time heard, or where a standard says so.
- **Every figure says what it rests on.** Thin data is shown with a caveat, not hidden and not padded.
- **One noise model.** Every ± comes from `confidence::standard_error`, with lag-one correlated readings counted as fewer. The CUSUM, change finder, isochronism and cross-correlation read ρ through the same core function (decided, R4, #647, #651): a mean's σ through `confidence::long_run_sd`, a slope's ± through Bartlett's factor (1 + ρx·ρres)/(1 − ρx·ρres) with ρx·ρres (corrected for noise in x as under Isochronism); only what is taken out before ρ is read differs.
- **True time.** Readings, windows and hours are on the take's one clock (Architecture Guide section 5).
- **Round once, at the end.** Core keeps full precision. Only the words layer rounds.

---

## 2. Terms

Each word below means exactly this, everywhere: in the code, the docs, the app and this guide.

### Takes and sound

| Term | Meaning | Where |
|---|---|---|
| **Take** | One recording of one watch: a folder with its sound, its `session.toml`, and any clock log. | `take.rs` |
| **Entry** | One line of a take's `session.toml`: the part files it reads, a position, and optionally a start, end, settle time and measure time. | `session::RecordingEntry` |
| **Part file** | One sound file of a take. A take's part files are read end to end as one sound. | `take::sound_files` |
| **Sound card** | The microphone's converter that turns sound into samples. Its crystal sets audio seconds. | `capture.rs` |
| **Sample** | One measurement of the microphone signal. The sound card takes a fixed number a second, its sample rate. | `audio.rs` |
| **Sound** | The samples of a take's part files, in order. | `audio.rs` |
| **Position** | How the watch lies: dial up or down, or crown up, down, left or right (DU, DD, CU, CD, CL, CR). | `session::Position` |

### The watch's figures

| Term | Meaning | Where |
|---|---|---|
| **Beat rate** | The beats an hour the watch is made to give, for example 28,800. Its nominal beat period is 3,600 s over the beat rate. | `BeatLog::bph` |
| **Rate** | Seconds a day the watch gains (+) or loses (−): the nominal beat period over the period the rate fit finds, less 1, times 86,400. | `timing.rs:163` |
| **Amplitude** | How far the balance swings from rest, in degrees. Worked out from the time between the unlock and the drop, the lift angle and the beat rate. | `amplitude.rs` |
| **Beat error** | Half the difference between the two alternating intervals, Tick to Tock and Tock to Tick, in ms. It is signed: + when Tick comes late. It is read from the unlock, else from the rate fit at the drop. | `session::beat_error_signed` |
| **Lift angle** | The angle the balance turns while the escapement drives it. It comes from the take or its calibre, else 52° is assumed with a caveat. | `take::DEFAULT_LIFT_DEG` |
| **Caveat** | A plain statement of what a reading rests on: short of beats, masked time, no clock correction, an assumed lift angle. | `measure::Caveat` |

### Beats

| Term | Meaning | Where |
|---|---|---|
| **Beat** | One tick or tock found in the sound. It has a time (its drop, on audio seconds), a number (counted from the start, skipped beats included), and a quality (how well it matches the take's usual beat; about 1 for a typical beat). | `beats.rs:74` |
| **Good beat** | A beat with quality above 0.4. Only good beats are timed or measured. | `Beat::is_good` |
| **Tick, Tock** | The even and the odd numbered beats: the watch's two sides. | `stream.rs:87` |
| **Unlock, drop** | The first and last sounds of a beat. The time between them gives amplitude. | `amplitude.rs` |
| **Jitter** | How far beats scatter about the rate fit: a robust SD, in µs. | `timing.rs:163` |

### Stretches of a take

| Term | Meaning | Where |
|---|---|---|
| **Settling period** | The first seconds of an entry, not read while the watch settles in its position: the entry's `settle_s`, else 20 s, but never more than half the entry. | `take::measured` |
| **Measured stretch** | The rest of the entry, from the end of the settling period to its stop, or for `measure_s` if one is set. | `take::reading_window` |
| **Position change** | A new entry in another position. It starts its own settling period, and is a break. | `clean.rs:44` |
| **Average Over** | The seconds a live reading covers, chosen in the app: 2, 4, 10, 20, 30 or 60 s. | app `app.rs:1698` |
| **Span** | The stretch one reading covers. Live: the last Average Over seconds, back to the latest cut. Saved: the measured stretch. | `live.rs:838`, `session.rs:449` |
| **Whole stretch** | The beats between two cuts. Each is fitted on its own. | `Cuts::piece_cuts` |

### Cuts: what no reading crosses

One idea, one word (decided 29 Oct, R7): every entry below is a **cut** of some kind. A cut has a kind and a width. Two rules cover them all: nothing is fitted, windowed or binned across a cut, and only a gap adds time. Code names (`clean::Hole`, `GAP_S`) move to this word as their files are touched.

| Term | Meaning | Where |
|---|---|---|
| **Cut** | Any place a reading may not cross: a gap, break, clock step, mask, position change, dropout or pause between beats. A cut has a kind, a start and an end on true time, and may have no width. | `clean::Hole` |
| **Pause** | The recorder stopped listening and started again. Pauses are listed in the take's `session.json`. | `breaks::from_session` |
| **Gap** | The time a pause lasted on true time: from the last sound before it to the first after it. A gap's length is known. | `clean.rs:153` |
| **Jump** | A sudden step in the beats' timing, found in the beats themselves (a knock, a lost stretch of sound). | `beats.rs:946` |
| **Break** | Any point where beats cannot be counted straight through: a pause, a jump, a position change, or a step of 0.5 s or more between the sound card and the take's clock. Beats are renumbered across it. | `breaks.rs:34` |
| **Clock step** | A step in the clock log that no listed pause explains. | `clean.rs:172` |
| **Mask** | A stretch the user marks by hand to leave out. Its beats and windows are left out, and its time is not counted as read. True time runs on through it. | `session::mask_log` |
| **Dropout** | Missing beats that no break explains. A 2 s part with no beat leaves its 10 s rate window out. | `clean::covered` |
| **Pause between beats** | More than 2 s with no beat (`GAP_S`). Live readings start again after one. See #638. | `clean.rs:38` |

### Windows, readings and series

| Term | Meaning | Where |
|---|---|---|
| **Rate fit** | One straight line through every good beat's time against its number, with Tick and Tock offset by the beat error. Its slope gives the rate. It is fitted twice more without beats over 5 robust SD from the line. | `timing.rs:163` |
| **2 s window** | 2 s of beats, laid end to end. For each side, the sound of its good beats (3 or more) is averaged into one shape, and the unlock and drop are found on it. It gives amplitude for Tick and Tock, and beat error from the unlock and from the drop. | `amplitude.rs:296` |
| **10 s rate window** | A rate fit over 10 s of beats, one every 5 s, so neighbours overlap by half. It needs a beat in every 2 s part and no cut. It gives the rate's ±, its 5% to 95% band, jitter and the rate chart. | `Cuts::windows` |
| **Reading** | The figures over one span: rate from the rate fit; amplitude and unlock beat error as the median of its 2 s windows; each with its ± and caveats. | `session::measure_masked` |
| **Bin** | A 10 s piece of true time, laid end to end. A bin is whole when it reaches into no cut and every 2 s part holds a beat. Power Reserve counts whole bins as the time read. | `Cuts::bins` |
| **Series** | Readings in time order, for charts and Steadiness tests. | `steadiness.rs` |
| **Grid step** | The spacing of an even series. For long runs: 0.25 s up to 12 h, 0.5 s up to 48 h, then 4 s. For Steadiness: the reading length chosen. | `longrun::timing_step` |

### Statistics

| Term | Meaning | Where |
|---|---|---|
| **Median** | The middle value, or the mean of the middle two. | `dsp::median` |
| **Percentile** | Linear interpolation between the two nearest values (decided 29 Oct). Two definitions in the code until #512. | section 8 |
| **Robust SD** | 1.4826 × the median distance from the median. It equals the standard deviation for normal noise and ignores a few wild values. | `dsp::robust_sd` |
| **±** | The standard error of a reading: how far it would likely move on a repeat under the same conditions. Not accuracy. | `confidence::standard_error` |
| **Lag-one correlation, effective readings** | How much each value resembles the next. Correlated values count as fewer independent ones in the ±. | `confidence.rs:66` |
| **Coverage** | Beats found over beats expected; unlock found over 2 s windows. | `Measurement` |

---

## 3. Time

A take has several clocks. Every beat is stored on audio seconds. Every reading is made on true time.

| Time | What it is | Used for |
|---|---|---|
| **Audio seconds** | Samples counted, over the sound card's sample rate. It runs as fast or slow as the card's crystal, typically off by 10 to 50 ppm (1 to 4 s/d). | Every stored beat and window time |
| **System time** | The computer's clock, kept to UTC by NTP. | The clock log; when a take started |
| **True time** | Audio seconds mapped through the take's clock, with each gap added. Seconds from the take's start. | Every reading, window, cut and hour (`Cuts::true_time`) |
| **Wall time** | Audio seconds with each gap added, and no clock correction. | The paper strip today (`breaks::wall`). A breach of one clock everywhere: it moves to true time (N9, #643). |
| **Watch time** | A beat's number times the nominal beat period: time as the watch's own beats count it. | Long-run period search (`longrun.rs:197`) |

### The take's clock: corrections, in the order picked

The **take's clock** is the one clock a take's readings are made on, picked once from these. Decided 29 Oct (clock order A): with no clock log, the USB lines win over a stated or kept correction. The code still picks the kept correction first until #635.

| # | Source | What it corrects |
|---|---|---|
| 1 | **Clock log rated by USB lines** | The log gives the pauses and steps; the USB lines give the card's steady error. |
| 2 | **Clock log** (`clock.csv`): pairs of audio seconds and system time, written while recording | A fitted line, so drift and steps are followed. |
| 3 | **USB lines**: faint 1 kHz lines a USB microphone picks up from the computer's bus | One steady error, read from the sound itself. |
| 4 | **Known correction**: `card_ppm` in `session.toml`, else the one kept for that input (`clocks.json`) | One steady error over the take. |
| 5 | **None** | Readings stay on the card's clock, with a caveat. |

- **One clock per take**, picked by `take::clock_from` (take.rs:51). The one exception is `regress`, which reads on the card's clock to match its references.
- **Signs:** ppm is positive when the sound card runs fast, (f − f₀)/f₀, the usual convention (decided 29 Oct; the code flips from today's + slow before release, #657).

---

## 4. How a figure is built

Each layer is made only from the layer above it. Rate skips the windows: it comes straight from the beats.

```text
Sound: part files, audio seconds
  ↓ found by the stream engine
Beats: time, number, quality; good beats only from here on
  ↓ moved onto true time; cuts and masks applied
Rate fit: one line through every good beat in the span, per whole stretch; gives the rate
10 s rate windows: a rate fit every 5 s; gives the rate's ± and band, jitter
2 s windows: each side's beats averaged into one sound shape; gives amplitude, unlock beat error
  ↓ rate as fitted; amplitude and beat error as the median of the windows
Reading: the figures over one span, each with its ± and caveats
  ↓ readings put together
Positions Test: mean per position, then X, largest, spread
Power Reserve hours: one reading per hour
Steadiness: mean of fine windows per reading today; core readings after #656
Long runs: median per grid step
```

- **Rate** is the fit over every good beat in the span (`timing.rs:163`). Across several whole stretches, the fits are averaged weighted by beats used (`timing.rs:230`).
- **Why 2 s windows:** one beat's sound is too faint to place its unlock and drop. Averaging each side's few beats makes the edges findable, while still following an amplitude that changes.
- **Why a median of windows:** an edge marked on the wrong sound moves one window, not the reading. A window short of a beat counts as one window. On 9 corpus takes, weighting windows by their beats moved amplitude 0.05° at most.
- **Beat error** is from the unlock (median of 2 s windows), else from the drop (the fit). `session::beat_error_signed` picks, and every surface shows that one. Decided (R6, #658): it says which it came from.
- **A window's amplitude** is the mean of tick and tock. If one side is missing, the other side alone is used today (`amplitude::mean_of_sides`). Decided: such a window is left out of the amplitude reading (N6, #648).

---

## 5. Window membership

**Rule (decided 29 Oct):** a window belongs to the reading whose span holds its midpoint. Each window is in exactly one reading of a tiling, and no window that reaches into a cut is used.

**Lives in:** one core function, to be written by #632 and #636.

Today there are five rules (#636):

| Where | Rule today | Status |
|---|---|---|
| Live cards, trend chart (`live.rs:866`, app `app.rs:1853`) | Window ends inside the span | **#632** |
| Saved session, whole take, Positions (`session.rs:500`) | Window wholly inside | **#632** |
| Steadiness (`steadiness.rs:2515`) | Grouped by grid time, every window required | **#636** |
| Long runs (`longrun.rs:219`) | Grouped by midpoint, windows across breaks kept | **#636** |
| App Distributions pane (`app.rs:15307`) | Its own filter, in the app | **#636** |

---

## 6. Gaps, masks and breaks

**Rule:** every cut is a hole on true time. No fit, window or bin reaches into one. A bin that partly overlaps a hole is left out. Each stretch between holes is fitted on its own.

**Lives in:** `clean::Cuts` (clean.rs:74), the one place cuts are decided.

| Kind of cut | What makes one | Where |
|---|---|---|
| Gap | A listed pause; its length is added on true time, except where a clock log shows no gap at it (then none) | `clean.rs:153` |
| Break | A clock step of 0.5 s or more, a jump in the beats, a new position | `breaks.rs:34` |
| Clock step | A step in the clock log that no listed pause explains | `clean.rs:172` |
| Mask | A stretch masked by hand | `session.rs:352` |
| Pause between beats | More than 2 s with no beat (`clean::GAP_S`) | `clean.rs:38` |
| Dropout inside a bin | A 2 s part of a 10 s bin with no beat; the bin is left out | `clean.rs:392` |

- **A listed pause the clock log saw no step for** is a break of no width: it adds no time, and the caveat `pauses_unseen` says so (`ClockFit::keeps_gap`; #646).
- **Short stretches are dropped.** A stretch between cuts whose beats span under 10 s (`timing::MIN_PIECE_S`) gives no rate. It is a ragged edge, and a few seconds of beats give a rate several s/d out.
- **A beat error that turns over across a break** is combined as a size, not averaged to zero (`timing::signed_across`, `median_signed`), with a caveat.
- **Breach:** the saved whole-stretch fit does not cut at a pause between beats over 2 s, though the live reading does (`Cuts::run_cut`) and `GAP_S`'s own comment says no reading fits across one. Finding N2, #638.

---

## 7. Minimum data

What each figure needs before it is shown today, and what happens when data is missing. Decided (R3): these counts give way to a gate on each figure's ±; this section is rewritten after the four method fixes. Counts stay only where a method breaks below them.

| Figure or step | Needs | With less | Where |
|---|---|---|---|
| Any timing fit | 6 good beats | No figure | `timing.rs:99` |
| Rate from a stretch between cuts | beats spanning 10 s | Stretch left out | `timing.rs:211` |
| 10 s rate window | a beat in every 2 s part, no hole | Window left out | `clean.rs:326` |
| Amplitude, one side of a 2 s window | 3 good beats | That side missing | `amplitude.rs:353` |
| Amplitude reading | 1 window | No figure | `session.rs:583` |
| Any ± | 3 values | No ± | `confidence.rs:129` |
| Beats found | 95% of expected | Caveat, figure kept | `confidence.rs:18` |
| Span heard | 10 s | Caveat, figure kept | `measure.rs:33` |
| Positions Test row | 10 s in position | No row | `session.rs:1026` |
| Distribution | 3 values | None | `distribution.rs:125` |
| Isochronism bin | 10 readings | Bin left out | `steadiness.rs:2633` |
| Steadiness reading | every fine window in it (until #656) | Reading left out | `steadiness.rs:2515` |
| Clock correction | 10 log entries, SE ≤ 20 ppm | Card clock, caveat | `clock.rs:169` |
| Power Reserve stop | 600 s with no run of 8 good beats | Run goes on | `power_reserve.rs:24` |

- **Breach (decided 29 Oct):** no rule yet ties a reading to the share of its windows that were usable; see Decisions. #645.

---

## 8. Medians and means

| Figure | Statistic | Why |
|---|---|---|
| Rate | Least-squares fit over beats, robust to outliers | Uses every beat, cancels beat error (Witschi) |
| Rate over several stretches | Mean of fits, weighted by beats used | The rate over the time heard |
| Amplitude, beat error (unlock) | Median of 2 s windows | A mis-marked edge moves one window, not the figure |
| Window amplitude | Mean of tick and tock | One number per window |
| Positions Test per position, X | Mean | As NIHS 93-10 defines the positions values |
| Steadiness bins | Mean of fine windows today | Decided (R2, #656): the core reading, so Steadiness and the charts agree |
| Long-run grid | Median per step | Period search on a robust series |
| Spread of residuals, jitter | 1.4826 × MAD | A robust standard deviation (`dsp.rs:253`) |

- **One median.** `dsp::median` (dsp.rs:215): the middle value, or the mean of the middle two. `twostate.rs:166` keeps its own copy (same result), and `session`, `analysis` and `twostate` each wrap it again. (#512)
- **Two percentiles.** Nearest rank in `analysis.rs:211`, `session.rs:394`, `longrun.rs:97`. Interpolated in `histogram.rs:100`, `distribution.rs:116`, `steadiness.rs:2635`. (#512; linear interpolation decided)

---

## 9. Uncertainty

**Rule:** every ± is `confidence::standard_error` of the values in time order. Scatter from successive differences (1.4826 MAD / √2), lag-one correlation off a fitted line, effective count n(1 − ρ)/(1 + ρ), then 1.2533 s / √(1 − ρ) / √n_eff. Full model: `docs/measurement-precision.md`.

**Lives in:** `confidence.rs:129`. Rate uses every other 10 s window today (`confidence.rs:144`); decided (R1, #636), the ± is read from the tiling of 2 s cells. Live scales it to Average Over (`measure.rs:415`).

- **What it is:** an estimate of precision from the take itself, under an AR(1) noise model. It is not accuracy: lift angle and clock are caveats, not part of the ±.
- **Coverage test:** a seeded test holds coverage between 55% and 85% at ρ 0, 0.5 and 0.9 (`standard_error_allows_for_correlated_readings`).
- **Breach:** where the beat error turns over, the figure is a median of sizes, but its ± is taken from the signed values, so the turn widens the ±. Finding N1, #637.
- **Gapped takes:** the windows of all stretches are put end to end, so one difference spans each gap, and ρ is not pooled per stretch, though `confidence::pooled_lag_one` exists for this. Finding N3, #639.
- **Steadiness:** the CUSUM assumes independent readings (`steadiness.rs:991`, `cusum.rs:11`). Decided: the step-robust long-run σ (#647), and the same for the change finder (#649).

---

## 10. Clock rules

**Rule:** one clock per take, picked once in the order of section 3. Every reading is made on it.

**Lives in:** `take::clock_from` (take.rs:51). The fit maps each beat onto true time before any reading (`clean.rs:187`).

- **A clock log is applied** only from 10 entries with an SE of 20 ppm (1.7 s/d) or less (`clock.rs:169-176`).
- **Sources disagree** when the USB lines and the log are 5 ppm (0.43 s/d) and 2 SE apart (`clock.rs:160`). With no clock log, the USB lines are weighed against a stated or kept correction instead: the caveat fires when the two are more than 5 ppm apart, with no SE test, since a correction carries no scatter of its own (decided 29 Oct, #635). #635 brings the code to the order in section 3.
- **ppm to s/d:** 1 ppm is 0.0864 s/d (`clock::rate_error_s_per_day`). A rate is corrected exactly, (1 + r/86400)/(1 + ppm·10⁻⁶) (`clockstore.rs:70`).
- **Live** corrects the rate by the mean ppm over the span, after the fit (`live.rs:961`). For a straight-line clock this matches mapping each beat.
- **Breach:** Power Reserve hours are cut every 3,600 s of sound-card time, not true time (`power_reserve.rs:255`, #642).

---

## 11. Rounding and units

| Figure | Unit | Shown to | Sign |
|---|---|---|---|
| Rate | s/d, + gains | 0.1 s/d | Always; zero has none (`library::signed_text`) |
| Rate p05 to p95 | s/d | 1 s/d | Always |
| Amplitude | degrees | 1° | None (whole degrees; CSV 0.1) |
| Beat error | ms | 0.01 ms | Size shown; signed in JSON (even beats late is +) |
| Jitter | µs | 1 µs | None |
| Clock | ppm, + card fast (#657) | 0.1 ppm | Always |

- **Rate:** (P_nominal / P − 1) × 86,400, exact, with P the fitted beat period (`timing.rs:163`).
- **JSON is never rounded.** Only text is rounded, once.
- **Today the decimals are typed in each surface**: six `opt` helpers, three `signed` helpers, and the app and MCP each pick when a ± shows (app.rs:12389, 12495, 12545; MCP summary.rs:12). The CSV writes more decimals than the text, which is right for data. Core giving each figure's decimals is #510.
- **Synthetic takes:** the generator steps each beat by P/(1 + r/86400) (`timing::period_s`, through `synth::period`), the period the timing fit reads r from, so a take made at +100 s/d reads +100 s/d (#816; finding N5).

---

## 12. Method audit

Each pane's method, the assumption it rests on, and whether that holds for a mechanical watch. Watch readings are not independent: on the corpus, neighbouring 2 s amplitude windows correlate by ρ 0.01 to 0.90 (0.70 to 0.90 on three takes) and 10 s rate readings by about 0 to 0.65. **Run** means a seeded simulation was run; **Read** means the code was read only.

| Pane | Method | Holds for watch data? | Evidence |
|---|---|---|---|
| Rate, ± | Least-squares fit of beat times; ± from neighbour differences with lag-one n_eff | **Yes** Correlation is allowed for | Read |
| CUSUM | Band 1.36 on short-term σ | **No** False alarms 85% at ρ 0.5, 100% at ρ 0.9 (5% meant) | Run, #647 |
| Change finder | Penalised segmentation of block medians, σ from neighbour differences | **No** Same flaw as the CUSUM. On 2 h of steady readings it finds a change that is not there in 1% of runs at ρ 0, 10% at ρ 0.5, 84% at ρ 0.8, 100% at ρ 0.9 | Run, #649 |
| Autocorrelation, Ljung–Box | Correlation after taking off the mean only | **Partly** A slow trend (amplitude falling over a long take) reads as memory and the test always rejects | Read, #650 |
| Isochronism | Rate median per 2° amplitude bin, weighted line and quadratic | **Partly** Noise in each amplitude reading shrinks the slope toward zero, and anything else that changes with time (temperature, position) is credited to amplitude. True −0.5 s/d per degree reads −0.11 on 30 min with a 3° fall, −0.29 on 2 h with 6°, −0.45 on 44 h with 15° (1.5° reading noise assumed). No ± | Run, #651 |
| Spectrum (FFT) | Hann-windowed FFT of the whole grid | **Partly** Empty readings are set to the mean, so line heights fall with the share missing. Welch, beside it, uses only unbroken stretches | Read, #652 |
| Period search | Spectrum divided by the AR(1) noise shape (ρ by `long_run_rho`, as everywhere) and, under the running median, by the median's own noise shape for periods under its window; then levelled by a local median; harmonics summed; false alarm corrected for the number of trials | **Yes** Seeded red noise (ρ 0 to 0.9, 1 min to 2 h) names a cycle in 0 to 11 of 100 takes per cell, against up to 35 before; the steady pane names something on 8 of 200 control takes. A sine on 1 min is found less often (85 against 136 of 1000), as the sine lifts the ρ read from 60 points | Run, #755, #761 |
| Cross-correlation | Rate against amplitude after a 30 min trend is taken off; n_eff from ρ_xρ_y | **Partly** The test allows for correlated series, but its slope shrinks with amplitude noise as the isochronism slope does | Read, #651 |
| Allan | Overlapping Allan deviation, full blocks | **Yes** Made for clock noise | Read |
| Two states | Two-normal mixture on 10 s block medians after a ±300 s running median, with separation, switching and share guards | **Yes** The fit assumes independent blocks, but the guards stop a correlated wander passing | Read |
| Distributions | Freedman–Diaconis bins, peaks over 3 bins, normal line from IQR/1.349 | **Yes** No false second peak in any run on one-peaked noise, ρ 0 or 0.9, 180 to 3,600 readings | Run |
| Wind-down | Median of a 30 min stretch at the start and at 24 h | **Yes** Medians resist the odd bad reading; the change has no ± | Read |
| STFT | 20 min Hann windows stepped 5 min, line taken off, only windows with every reading present | **Yes** Nothing filled in | Read |
| Wavelet (SST) | Morlet transform on each unbroken stretch, edge zone (cone of influence) left empty | **Yes** Nothing filled in across a gap | Read |
| Wavelet coherence | Read against the 95th percentile of red-noise pairs with each series' own ρ (Grinsted 2004), fixed seed | **Yes** Correlated noise is the reference | Read |
| Kurtogram | Spectral kurtosis over half-overlapping Hann windows; noise band 2/√windows | **Yes** Noise reads −2/windows rather than 0 (−0.23 at 8 windows, −0.03 at 64), against −1 for a steady line; the stated band is wider than the real spread, so it errs safe. Same at ρ 0 and 0.9 | Run |
| Long runs | Residuals of the whole-take fit, median per grid step on watch time (beat number × nominal period), then the period search | **Yes** On watch time a wheel's period is exact even as the rate drifts. Its own percentile is the nearest-rank copy (#512) | Read |
| Power Reserve | Ends after 600 s with no run of 8 good beats; each hour measured like a take | **Yes** It times the last beat heard, which near the end of the reserve may be a little before the last tick. Hours cut on sound-card seconds (#642) | Read |
| Positions Test | Means per position, NIHS 93-10 indices | **Yes** The standard's own method | Read |

Fixes decided 29 Oct: see Decisions (change finder #649, autocorrelation #650, isochronism and cross-correlation #651, FFT #652).

---

## 13. Findings

Confirmed problems are filed as issues, and the fixes go to each file's owner. None is fixed by this guide.

| # | Finding | Where | Status |
|---|---|---|---|
| N1 | Beat error ± from signed values when the figure is a median of sizes | `measure.rs:498`, `session.rs:579` | **#637** |
| N2 | Saved fit does not cut at a pause over 2 s between beats; live does | `session.rs:478`, `clean.rs:38` | **#638** |
| N3 | ± on a gapped take takes differences across gaps; ρ not pooled per stretch | `confidence.rs:144` | **#639** |
| N4 | No core coverage rule; the 60% unlock share lives in `regress` | `regress_cmd.rs:32` | **#645** |
| N5 | Synthetic rate is first-order: +0.12 s/d at +100 s/d | `synth.rs:171` | Fixed, #816 |
| N6 | A window's amplitude falls back to one side with no caveat; tick and tock can differ by degrees | `amplitude.rs:224` | **#648** |
| N7 | `confidence::beat_error_se` is used only by its test; `assess` repeats it inline | `confidence.rs:155` | **#641** |
| N8 | CUSUM assumes independent readings | `steadiness.rs:991` | **#647** |
| N9 | The paper strip places beats on wall time (no clock correction) and draws the Rate line at the corrected rate, so on a corrected take the two differ in slope by 0.0864 s/d per ppm (confirmed by building and running). One clock everywhere decides the fix: the dots move onto true time, in core. | `timing.rs:63`, app `strip.rs:200` | **#643** |
| N10 | A listed pause the clock log saw no step for still adds its gap to true time | `clean.rs` `Cuts::true_time` | **#646** |
| M1 | Change finder assumes independent readings | `steadiness.rs:1040` | **#649** |
| M2 | Autocorrelation and Ljung–Box take off the mean only, so a trend reads as memory | `steadiness.rs:946` | **#650** |
| M3 | Isochronism slope shrinks with amplitude reading noise, is confounded with time, has no ± | `steadiness.rs:2678` | **#651** |
| M4 | FFT sets empty readings to the mean; heights fall with the share missing | `spectrum.rs:218` | **#652** |
| R2 | Steadiness makes its own readings (mean of fine windows), not the core reading | `steadiness.rs:2515` | **#656** |
| R5 | Clock sign is + when the card runs slow, against the usual convention | `clock.rs:29` | **#657** |
| R6 | Beat error falls back from the unlock to the drop with nothing saying so | `session.rs:413` | **#658** |
| K1 | Two percentiles; median copies | six files | **#512** |
| K2 | Five window rules; live plain median of beat error | see section 5 | **#632, #636** |
| K3 | Decimals and ± thresholds typed in surfaces | app, CLI, MCP | **#510** |
| K4 | Power Reserve hours on sound-card seconds | `power_reserve.rs:255` | **#642** |

Checked and clean: the good-beat rule is one function (`Beat::is_good`); the CUSUM band is one constant in core; settle and reading window are one function each; masks and breaks go through `clean::Cuts` on every saved path.

---

## 14. Before a PR that touches math

- Every average, median, percentile and ± calls the one core function for it.
- Windows are picked by the one membership rule.
- No fit, window or bin reaches into a cut; nothing is filled in.
- A figure from thin data carries its coverage and caveat.
- A new estimator with a ± has a seeded coverage test, on null and on gapped data.
- Units and signs follow section 11, and nothing is rounded before the words layer.
- Any regress value that moves is listed in the PR body.
- A new Steadiness test or lead keeps the pane's seeded run on steady takes at 95 in 100 or better (R9).
- A statistic that assumes independent readings uses the one long-run σ (R4).

