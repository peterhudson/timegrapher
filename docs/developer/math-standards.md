# Math Standards Guide

Version 1, approved 2026-10-29. This guide says how every figure is computed: what is averaged, how, over what, what a gap does, how much data a figure needs, and what its ± means. Every PR that changes how a figure is computed is built to it and checked against it. Where today's code differs, the code changes, not the guide, and the issue that changes it is named. A case the guide doesn't cover is raised, and its answer is added here.

Its peers are the [Architecture Guide](architecture.md) (where a rule lives) and the [Design Guide](design/design-guide.md) (how it looks). Audited at main `8b7853e`. Code was read, not run, except where a line says a simulation was run. Labels: N and K are findings of the audit, M of the method audit, R of the red team that reviewed this guide before approval.

---

## Decisions (2026-10-29)

- **Window membership:** a window belongs to the reading whose span holds its midpoint (option B).
- **Percentile:** linear interpolation (Hyndman and Fan type 7, the default in R and NumPy), one core function: `dsp::quantile` on sorted values, `dsp::percentile` on any values (#866).
- **Minimum data:** each reading carries "windows used of windows in the span" from core. Below 60% it gets a caveat ("Read from 4 of 15 windows"). With fewer than 3 windows it shows no ±. The figure is still shown, never filled in, unless fewer than 10% of them give it: then the amplitude, Tick, Tock or unlock beat error is withheld, with the same caveat (`confidence::MIN_WINDOW_SHARE`, #1429). The 3-window floor is for the ± only. This holds until R3 replaces the counts in section 7 with a gate on the ±.
- **Partial windows:** a window short of some beats counts as one window in the median, as long as each side has its minimum of 3 good beats.
- **A listed pause the clock log didn't see:** with a clock log, the log decides. The pause is still a break (beats are renumbered and nothing is fitted across it), but it adds no time, and a caveat says the two disagree. Without a log, the listed gap is added.
- **CUSUM:** scaled by the long-run σ (#647). The scale is s × √(1 + ρ) ÷ (1 − ρ), with s the short-term σ from neighbour differences (for lag-one correlated readings s = σ·√(1 − ρ) and the long-run σ is σ·√((1 + ρ)/(1 − ρ))): 2.45 at ρ = 0.5, 4.4 at ρ = 0.7. ρ is step-robust: the lag-one correlation of the readings after each side's mean is taken off at the CUSUM's largest point, so a real step does not count as correlation, then bias-corrected as ρ̂ + 2(1 + 3ρ̂)/n and clamped to 0 to 0.99. ρ̂ is read only from neighbouring readings that are both present, so a gap never joins readings that were not next to each other, and n is the number of such pairs plus one (the count of readings when none is missing); read across gaps, or with n the readings present, half the readings missing at ρ 0.9 false-alarmed 19% and 10%, against about 5% this way. A wheel's cycle or two-state switching counts as noise: it raises ρ, so the CUSUM flags only a shift larger than the cycle, and the cycle and the switching show in Long runs and Two states. On a seeded test (AR(1), 360 readings, 2,000 runs) false alarms are 4 to 6% from ρ 0 to 0.9, against 85% at ρ 0.5 and 100% at ρ 0.9 today, and steps are caught close to the rate with ρ known. ρ taken from the differences alone false-alarmed 18% at ρ 0.9, and ρ from the raw readings hid real steps. It lands after the autocorrelation fix (#650). A level change is named by one rule, in the headline and on the chart alike: the change finder cut the series, the cut explains at least 20% of its variance, and the lead is not a slide (`steadiness::level_change`). The CUSUM's band is a guide to the eye; a CUSUM past it with no such change names none, and its corner is marked at that change's cut. A small step that only the CUSUM shows goes unnamed, though the line still bends (#697, #717). Whether the 20% share is the right bar is for section 7's rewrite (#721).
- **One-sided windows:** a 2 s window with one side read is left out of the amplitude reading, and kept in that side's Tick or Tock figure. Treating the missing side as equal to the other would be filling.
- **Change finder:** a step is judged against the same step-robust long-run σ as the CUSUM, from the one core function, on the block medians it works on; seeded test: at most 5% false changes on steady readings at ρ 0.9, and a 2 s/d step still found (#649). Step or slide: the cut series is a slide when its line, trend test passed, explains at least as much as the stretches do, and a step otherwise (decided 4 Nov, #721). The bar was 0.8, and a line explains a clean mid-take step about 0.75 as well. On 720 readings at ρ 0.9 (200 seeds a cell; takes cut and with the trend test passed), 2σ and 3σ steps called slides fell from 82 of 181 and 73 of 200 to 6 and 4. The cost is ramps: a 4σ ramp the finder cuts is called a level shift in 24 of 161 (1 before), and a 2σ ramp in 32 of 54 (10 before). At ρ 0.5 no step is called a slide (41 and 20 before), and ramps lose 19 of 148 (2σ) and 7 of 200 (4σ).
- **Autocorrelation and Ljung–Box:** the slow trend is taken off first as the period search takes it off (`longterm::detrend` with `DETREND_S`): a running median over 30 min or a quarter of the take, whichever is shorter, and a quadratic fit on a take under 20 min, so a wind-down does not read as memory while a wheel's cycle (60 s for the fourth wheel) still shows; seeded test both ways (#650). Each lag is then read less what white readings read once detrended the same way, and Ljung–Box is read against those readings rather than the plain χ² (#1199; `steadiness_white.rs`, table `data/white-null.json`). The nulls are drawn offline, on gap-free white readings detrended exactly as a take is: a quadratic under 20 min, by readings (30 to 1,199), and a running median by its window (5 to 1,800 readings) and the take's length over it (4 to 8 windows: a quarter of the take up to 2 h, then 30 min of up to 4 h, past which a take reads as its first 4 h). Each point keeps the mean correlation at lags 1 to 128 (the bias taken off: about −3/n at lag one after a quadratic, as `confidence::fit_lag_bias` gave before, #828, and about −0.7/window under a running median, which was left in) and Q's mean, variance and skew over 10 lags, from 40 million readings drawn (1,000 to 40,000 series): one null serves every take of its length, so an error in it is shared by all of them. 200 series read 71 of 1,000 white takes of 1 h at p 0.05, and at 8 million readings a point takes of 83 min read 1,106 of 20,000. A take reads the detrend its own grid got, gaps included, at its present readings, interpolated in log n (or by their share of the window, then in log window); outside the table, or with another number of lags, the plain χ². Q is read against a shifted, scaled χ² matched to the three moments (Pearson); two left the 0.004 tail about twice too heavy at 20 min and 1 h. A take with gaps reads the gap-free null of its present readings, on the detrend its own grid got: a take of 25 min with 5 min missing reads the running median's null, not the quadratic's. Before, on 4,000 white takes of 10 s readings, the plain χ² named 377, 131 and 75 (200, 40 and 16 expected) at 20 min and 283, 73 and 37 at 30 min: the running median's bias, and Q's own excess spread on short takes (1.3 to 1.6 times at 0.01 even with only the mean out). Against the table, on two fresh sets of seeds (4,000 takes a row each), every row held its upper 99% bounds of 236, 56 and 26: gap-free takes of 10, 15, 20, 25, 30 min, 1 h, 83 min, 2 h and 8 h at 10 s and of 10 and 30 min at 2 s; 15, 30 min and 1 h with gaps (scattered single readings, one gap, three blocks); and takes over 20 min with under 20 min present (25 min with one 25% gap, 22 min with 15% scattered). The worst counts were 233, 56 and 24. The cost in power: AR(1) at ρ 0.1 over 30 min is named at p 0.01 in 114 of 4,000 against 168 with the plain χ², at ρ 0.2 in 677 against 820. On 300 white takes of 30 min the pane called a series wandering on 4, against 11 before (on the table's first draw, 8 million readings a point). The 2 s windows a repeat is looked for in, whose p nothing reads, keep the plain χ². Ljung–Box weighs each lag by its pairs, r²·pairs·(n + 2)/n summed, which is the usual Q with every reading present; the usual sum over all n read Q up to n²/(n − k)² too high where gaps thin the pairs.
- **FFT:** the Hann-windowed FFT stays for a take with no gaps. The Hann window tapers the two ends; it does not fill gaps. A take with gaps uses Lomb–Scargle with the same Hann weights: at each period, the sine fitted by weighted least squares to the readings that exist, nothing filled in. On a complete take this is the Hann FFT's size within 0.5%, except at the longest periods, where the fit's 2ω term (the sine's own overlap with the window) moves a bin by a few percent, +4% at 683 s on 900 readings; one reading out leaves the spectrum as it was; an unweighted fit moved the peak of a nearly complete take (683 s to 410 s) and single bins ×0.04 to ×6 (#652, #683).
- **Isochronism:** every 10 s reading is kept (no smoothing over time, so a swing over minutes from a wheel still shows). Each reading's amplitude noise comes from the scatter of its own 2 s windows, and the slope is corrected for it (errors in variables). It is withheld when amplitude changed too little to tell, and carries a ±. The ± is Fuller's (1987, §1.2) variance for a slope with noise in x: residuals from the corrected slope, the true x's variance m̂XX = mxx − σuu (σuu the mean reading noise), and Bartlett's factor on the equation-error term σee/m̂XX only, with ρX = ρx/λ and ρe = ρv·σvv/σee (white reading noise dilutes the lag-one ρ read from the readings; each ρ through `confidence::long_run_rho`, capped at 0.99). With no noise in x it is Bartlett's factor with ρx·ρres. Run: ±1.96 SE holds the true slope 929 to 991 times in 1,000 across ρ 0, 0.5 and 0.9, x noise 0 to 3.5° on a 5° spread, and no gaps, blocks or half missing; Bartlett's factor on all of se/λ held as few as 816 (#651). The cross-correlation slope gets the same correction. Caveats: listed position changes in the take; rate and amplitude also move with anything else that changes over the take and is not recorded. Simulated: true −0.5 s/d per degree reads −0.50 on 2 h of ±5° swings, against −0.40 today (#651). To check on real takes: that noise in neighbouring 2 s windows is independent.
- **From the red team (R1 to R8):** one tiling of 2 s cells on true time, with a 10 s reading as five cells and the ± read from the tiling; Steadiness readings from the core reading function; figures gated by their ± rather than by counts; one core lag-one ρ (`confidence::long_run_rho`), with its bias correction and gap pairing, behind the ±, CUSUM, change finder, isochronism and cross-correlation: `confidence::long_run_sd` turns it into the long-run σ of a mean for the CUSUM and change finder, and a slope's ± uses Bartlett's factor, n_eff from ρx·ρres (with noise in x, on the true x's term only: see Isochronism); with what is taken out before ρ is read named by the caller (a line for the ±, the best single step for the CUSUM and change finder); the clock sign flipped before release so + ppm means the sound card runs fast; beat error says whether it came from the unlock or the drop; one word, cut, with a kind and a width; decimals follow the ± (in text, replaced on 12 Nov by section 11's fixed places). Filed or noted: R1 on #636, R2 #656, R4 on #647 and #649, R5 #657, R6 #658, R8 on #510. R3 and R7 are this guide's: R7 is in section 2; R3 rewrites section 7 after the four method fixes.
- **Is a watch heard (#675, #678):** over 10 s or more, a timing scatter of 2% of a beat or more (`beats::NO_WATCH_JITTER_FRACTION`) is no watch, whatever the sound. From 0.8% (`beats::NOISE_DOUBT_FRACTION`) up to it, where stand noise can still fall and so can a weak watch about to stop (up to 1.4% in a take's last 25 minutes), and on any stretch under 10 s, the timing says a watch unless the beats' averaged sound shows no unlock or drop on either side; a sound that can't be judged does not veto. The sound is judged from beats sampled evenly across the whole stretch (one sample of the take's beat sound in core, a fixed budget whatever its length), and can't be judged when those span under half the stretch or neither side has three (#1250). Every reader asks `measure::beat_sound_needed` and decides with `measure::no_watch_reason`. Run: stand noise is read as a watch on about 1 in 70 seeded draws of 30 s and 2 in 60 of 90 s; the test names them and the list only shrinks. Decided 30 Oct: a dying watch keeps its readings at that cost. Just after a jump in the beats, until a whole 10 s window fits after it, the stretch counts as under 10 s: the windows skip the jump, and having none is not a verdict of noise (#1170).
- **Size of a cycle (#761):** a cycle's peak to peak is read from its folded shape smoothed to its first harmonics: no more than the search summed, no more than the whole cycles in the take, and each one past the fundamental only if it stands out of the AR(1) noise at its own frequency (1% χ² with 2 degrees of freedom). Since 13 Nov the shape is the least-squares sines the search takes out, not the fold's medians (section 8); the ×π/2 is kept, which makes the bar conservative. On 2.6 cycles a 225 s line read 29.4 s/d with eight harmonics against 10.6 with the rule; on a seeded 60 s sine in ρ 0.8 noise the median swing is 1.06 of true against 1.39 with every harmonic kept. Decided 31 Oct.
- **Whole fractions (#787, #789):** a line is n of a longer period (a wheel's turn, or a found cycle's harmonic) when its frequency is within one resolution (1/span) of n over that period (`longterm::harmonic_offset` under 1). Neighbouring n are one over the period apart, so when the period is longer than half the span two can fit; then no fraction is told. Wheel fractions follow this since #787, and a found cycle's harmonic label since #803. A wheel fraction is marked only when exactly one wheel and n from 2 to 6 fit. On a 580 s span, a 224.84 s line is 0.43 resolutions from half the 540 s third wheel's turn and 0.64 from a third, so it gets none. Decided 31 Oct.
- **Many tests (R9):** after the CUSUM, change finder and autocorrelation fixes, a seeded run of the whole Steadiness pane on a few thousand steady simulated takes (ρ 0 to 0.9, 30 min to 24 h) counts how often any lead shows. Target: at least 95 in 100 steady takes say nothing stands out. If missed, a lead must clear Holm's bar across the pane; each test's own plot and p stay. The run stays as a seeded test.
- **Displayed precision (Peter, 12 Nov):** no figure is ever written finer than its place in section 11 (amplitude 1°, rate 0.1 s/d, beat error 0.01 ms). The ± is written at the same place and rounded up, so a ± smaller than the place reads as one unit of it ("149° ± 1°"). On the reading cards the place is fixed, so a card's width never changes as its ± moves (Hugh). JSON and CSV keep full precision. This replaces "decimals follow the ±" (#510 K3) for text (#1470). See section 11.
- **Enough values for a ±:** a ± whose own reading holds fewer than 30 values takes its noise (the scatter and ρ) from the latest 30 values that end where the reading ends, with n from the reading itself. A spread read from 3 to 5 values is wild: with 5 values (a 10 s average of 2 s windows) about 1 reading in 70 reads under a tenth of the true ±, which made the cards flicker to 148.94° ± 0.01° (simulated, 200,000 draws). A saved reading takes the 30 nearest within its take, before it first and then after. With fewer than 30 values within an hour either side (so far, live), the reading has no ± (#1470). See section 9.
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
| **10 s rate window** | A rate fit over 10 s of beats, one every 5 s, so neighbours overlap by half. It needs a good beat (one the fit times from) in every 2 s part and no cut (#1432). It gives the rate's ±, its 5% to 95% band, jitter and the rate chart. | `Cuts::windows` |
| **Reading** | The figures over one span: rate from the rate fit; amplitude and unlock beat error as the median of its 2 s windows; each with its ± and caveats. | `session::measure_masked` |
| **Bin** | A 10 s piece of true time, laid end to end. A bin is whole when it reaches into no cut and every 2 s part holds a beat. Power Reserve counts whole bins as the time read. | `Cuts::bins` |
| **Series** | Readings in time order, for charts and Steadiness tests. | `steadiness.rs` |
| **Grid step** | The spacing of an even series. For long runs: 0.25 s up to 12 h, 0.5 s up to 80 h, then 4 s. For Steadiness: the reading length chosen. | `longrun::timing_step` |

### Statistics

| Term | Meaning | Where |
|---|---|---|
| **Median** | The middle value, or the mean of the middle two. | `dsp::median` |
| **Percentile** | Linear interpolation between the two nearest values (decided 29 Oct): `dsp::quantile` and `dsp::percentile`, one rule in core since #866. | section 8 |
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
| 4 | **Known correction**: `card_ppm_fast` in `session.toml`, else the one kept for that input (`clocks.json`) | One steady error over the take. |
| 5 | **None** | Readings stay on the card's clock, with a caveat. |

- **One clock per take**, picked by `take::clock_from` (take.rs:51). The one exception is `regress`, which reads on the card's clock to match its references.
- **Signs:** ppm is positive when the sound card runs fast, the usual convention (decided 29 Oct, #657). One second of a card `ppm` fast is exactly (1 − ppm·10⁻⁶) true seconds (`clock::card_scale`, the definition); the clock log's slope and the USB lines both go through it. Files written before #657 (`clocks.json` `/1`, `analysis.json` `/1`) are read with the sign changed; a session file's old `card_ppm` is refused.

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
Steadiness: median of the fine windows per reading, as the core reading takes it (#656)
Long runs: median per grid step
```

- **Rate** is the fit over every good beat in the span (`timing.rs:163`). Across several whole stretches, the fits are averaged weighted by beats used (`timing.rs:230`).
- **Why 2 s windows:** one beat's sound is too faint to place its unlock and drop. Averaging each side's few beats makes the edges findable, while still following an amplitude that changes.
- **Why a median of windows:** an edge marked on the wrong sound moves one window, not the reading. A window short of a beat counts as one window. On 9 corpus takes, weighting windows by their beats moved amplitude 0.05° at most.
- **Beat error** is from the unlock (median of 2 s windows), else from the drop (the fit). `session::beat_error_signed` picks, and every surface shows that one. Decided (R6, #658): it says which it came from.
- **A window's amplitude** is the mean of tick and tock. If one side is missing, the window is left out of the amplitude reading and kept in that side's own figure (`amplitude::mean_of_sides`; N6, #648).

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
| Gap | A listed pause; its length is added on true time, except where a clock log spans it and shows no gap at it (then none) | `clean.rs:153` |
| Break | A clock step of 0.5 s or more, a jump in the beats, a new position | `breaks.rs:34` |
| Clock step | A step in the clock log that no listed pause explains | `clean.rs:172` |
| Mask | A stretch masked by hand | `session.rs:352` |
| Pause between beats | More than 2 s with no beat (`clean::GAP_S`) | `clean.rs:38` |
| Dropout inside a bin | A 2 s part of a 10 s bin with no beat; the bin is left out | `clean.rs:392` |

- **A listed pause the clock log saw no step for** is a break of no width: it adds no time, and the caveat `pauses_unseen` says so (`ClockFit::keeps_gap`; #646). A pause the log has no entry before or none after (it ended early or started late) has no witness, so its listed gap is added, as with no log (`ClockFit::witnesses`).
- **A lead of noise before the watch is left out** of a whole reading (`analyze`, a take's entry), not of a set average (live or saved): the reading starts at the first 10 s window that is the watch's: its scatter under 0.8% of a beat (`beats::NOISE_DOUBT_FRACTION`), the windows from it on under it by their median for at least 20 s, and its scatter at most 4 times theirs, taken as at least 0.02% of a beat (`session::LEAD_SCATTER_RATIO`, `LEAD_SCATTER_FLOOR`; #1418); and the next window that doesn't overlap it passes the same tests, with no jump of the beats found from the first's start to the second's end and every 5 s window between them there, since one 10 s window of noise can scatter as little as a watch's (0.34% of a beat at 18,000 bph, under the line for a watch scattering 0.1%) but two apart have not been seen to; a pair across the step onto the watch, or across a window left out there, would set noise beside the watch's own windows. It starts there, at the beats' jump onto the watch when there is one, and the caveat `lead_left_out` says how much (`session::watch_lead_s`; #844). Noise on a stand scatters 1.5 to 4% of a beat, so its windows can otherwise outnumber the watch's in the verdict. A 10 s window of noise can still dip under 0.8%, to 0.7% at 18,000 and 19,800 bph, against the watch's 0.01%: the ratio keeps such a window in the lead. A watch with no lead whose first windows scatter more than 4 times its later ones (0.5% of a beat for 30 to 60 s, then 0.01%) has that rough start left out the same way, and named `lead_left_out`; its rate stays within 0.3 s/d (tests/noise_lead.rs).
- **Short stretches are dropped.** A stretch between cuts whose beats span under 10 s (`timing::MIN_PIECE_S`) gives no rate. It is a ragged edge, and a few seconds of beats give a rate several s/d out.
- **A beat error that turns over across a break** is combined as a size, not averaged to zero (`timing::signed_across`, `median_signed`), with a caveat.
- **Unlisted pauses (#638):** saved whole-stretch fits use `Cuts::fit_cuts`, including pauses of more than `GAP_S` between beats on true time. Live's `Cuts::run_cut` uses the same pause rule for its latest run. Saved rates pool the separate fits by beats used, retaining the existing minimum stretch length.

---

## 7. Minimum data

This section lists what each figure needs before it is shown, and what happens when data is missing. Decided (R3): these counts give way to a gate on each figure's ± once the four method fixes are in. Counts stay only where a method breaks below them.

### Rules for every threshold (Peter, 13 Nov)

Peter asked for these rules after the isochronism walk was found dropping 5 min bins below 50% of their windows, while the strip and cards keep figures down to 10%. Every such threshold belongs in core, and two thresholds for one purpose are acceptable only with a defensible reason.

1. **Named, in core, once.** Every threshold that withholds, culls, greys or caveats a figure is a named constant in core. The app, CLI and MCP read it and never restate the number. A literal in a comparison is a finding.
2. **One question, one threshold.** The question "is there enough of this figure's data?" gets the same floors on every surface. A summary read from windows or readings (a card, a table cell, a walk point, a bin, an hour) is withheld under `confidence::MIN_WINDOW_SHARE` (10%) of its nominal values, greyed under `confidence::MIN_WINDOW_COVERAGE` (60%), and has no ± under `confidence::MIN_VALUES` (3).
3. **Raw values and summaries.** A raw value, such as one 2 s window's amplitude or one 10 s reading's rate, is drawn whenever it exists. A summary over a span follows rule 2. That is why the strip can show points near the end of a run-down while the card for the same span shows a dash. This is the one defensible way two plots of the same data may differ, and the surface says so.
4. **A second threshold needs a reason a method gives.** A different floor is allowed only where the method breaks below it, for example a straight-line fit needs enough beats, or an autocorrelation needs enough pairs. Each row below states that reason. A number with no reason is a finding (13 Nov audit, below).
5. **Changing a threshold** is a math PR: it gets a Norm gate, it needs a corpus run listing every figure the change moves, and it updates this table.

### What each figure needs

| Figure or step | Needs | With less | Constant | Why this number |
|---|---|---|---|---|
| Any timing fit | 6 good beats | No figure | `timing.rs` `solve` (literal; finding T4) | Not recorded beyond the fit's 3 unknowns (finding T8) |
| Rate from a stretch between cuts | beats spanning 10 s | Stretch left out | `timing::MIN_PIECE_S` | One 10 s bin; a few seconds of beats give a rate several s/d out (its doc) |
| 10 s rate window | a good beat in every 2 s part, no hole | Window left out | `clean.rs` | The window tiling: a hole would make the window shorter than it says, and the fit times from good beats only, so good beats in only part of it would read the scatter of that part alone (#1432) |
| Amplitude, one side of a 2 s window | 3 good beats | That side missing | `amplitude.rs` | Not recorded (finding T8) |
| Amplitude, one side of a 2 s window, gate | 100° to 380° at the lift in use: at the first read, and again at every later lift from the side's kept unlock and drop, both ways (#1333) | That side missing, its edges kept so another lift can read it | `amplitude::MIN_AMPLITUDE_DEG`, `amplitude::MAX_AMPLITUDE_DEG` (`amplitude::side_deg`, `amplitude::at_lift`) | The low bound is not recorded (finding T8); above 380° the edges were marked on the wrong sounds (its doc). #1333 changes neither number, only applies them at the lift in use |
| Amplitude reading | 1 window, then the 10% floor below | No figure | `session.rs` | A median needs one value; the share floor does the real gating |
| Amplitude, one side of a 2 s window, sound 1 later than the stretch's | Where the side's template of every beat in the chunk finds the unlock beyond a 20 ms template's search (`amplitude::short_reach_s`, 14 ms), the window's sound 1 more than 2 ms after it (#1623) | That side missing, no edges kept: it is not timed again from a sound it did not find | `amplitude::UNLOCK_LATE_S` | In a 47 s chunk the unlock barely moves (a fall from 300° to 250° moves it 1.3 ms); near stopping the impulse sits about 6 ms after the unlock at 28,800 bph, and was read as it (142° for 100 to 105°, #1623) |
| Window figure (amplitude, Tick, Tock, unlock beat error) | 10% of the reading's nominal kept windows (#1429) | No figure, its caveat kept | `confidence::MIN_WINDOW_SHARE` | Under a tenth, a median is set by the few windows that found the sound, which near a run-down's end may be noise (#1429) |
| Any figure read from windows | 60% of nominal | Greyed, caveat with the count | `confidence::MIN_WINDOW_COVERAGE` | Set in R3; no reason recorded beyond the figure covering most of its span (finding T8) |
| Any ± | 3 values | No ± | `confidence::MIN_VALUES` | A robust spread needs at least 3 values |
| ± noise | 30 values, borrowed from the latest when short (decided 12 Nov, #1470) | — | `confidence::MIN_NOISE_VALUES`, `confidence::noise_pool` | A robust SD from 5 values is wild; about 30 make it steady (section 11) |
| ± noise, how far to borrow | 1 h either side of the reading (#1470) | No ± | `session::BORROW_LOOK_S` | Locality: noise from more than an hour away describes a different state of the watch |
| Unlock beat error, sign agreement | its windows' share on the median's side no more than max(0.18, 3 √(q(1 − q)/n)) short of p = Φ(\|median\| / robust SD), q being p or the observed share, whichever varies more; not judged with under 3 windows or no spread (#1439) | No unlock figure: the drop's beat error, its caveat saying the unlock was inconsistent | `confidence::MAX_SIGN_SHORTFALL`, `confidence::unlock_inconsistent` | Set on the corpus's healthy readings, with 3 of 3,084 falling back (#1458) |
| Beats found | 95% of expected | Caveat, figure kept | `confidence::MIN_BEAT_FRACTION` | Not recorded (finding T8) |
| Span heard | 10 s | Caveat, figure kept | `measure.rs` | One rate window |
| Positions Test row | 10 s in position | No row | `session::POSITION_MIN_S` | One rate window |
| Distribution | 3 values | None | `distribution.rs` | `MIN_VALUES` |
| Isochronism bin | 10 readings | Bin left out | `steadiness::ISO_MIN_READINGS` | A median of 10 has a ± about two fifths of one reading's spread (1.25/√10); the number itself is not recorded (finding T8) |
| Isochronism curve | 3 bins | No curve | `steadiness::ISO_MIN_BINS` | A curve through fewer than 3 bins is a line or a point |
| Isochronism slope (errors in variables) | reliability λ ≥ 0.5, 30 pairs | No slope, a caveat | `amplitude_slope::MIN_RELIABILITY` | Below λ = 0.5 the correction multiplies the slope by more than 2, so the noise in x sets the answer (#651) |
| Isochronism walk point | as any window figure: 10% withhold, 60% grey, per series (ruled 12 Nov) | Point left out or faint | `confidence::MIN_WINDOW_SHARE`, `MIN_WINDOW_COVERAGE` | Rule 2; it was 50% as a literal (finding T1) |
| Isochronism walk | 3 points | No card | `steady_views::WALK_MIN_POINTS` | Fewer can't show a direction |
| Isochronism walk, rate read with no amplitude past its end | 15 min (3 base bins) past the last point | No line in the facts box | `steady_views::WALK_RATE_ONLY_S` | The last point sits at its bin's middle and a take's last bin can be cut short, so a walk read to the end stops up to 1.5 bins early; 3 bins clear that edge by a whole bin, so the line names only a stretch the rate was read through with no amplitude (its doc) |
| Autocorrelation lag, Ljung–Box | 30 pairs | Lag not read | `steadiness` `MIN_PAIRS` | Below 30 pairs a lag's correlation strays by over a third under independent readings |
| Steadiness tests | 300 s of beats (30 rate readings) | Not run | `joined::MIN_S` | `MIN_PAIRS` worth of readings |
| Steadiness reading | every fine window in it, from the series' one source (#1364) | Reading left out | `steadiness.rs` | Readings are compared with each other, so each must cover what it says |
| Wheel lines: a size window or turn | 90% of its readings, no hole over 5 s | Left out | `wheel_lines` `MIN_SHARE`, `MAX_HOLE_S` | A fold averages by phase, and a hole puts one phase short; stricter than 60% because the shape, not a level, is read (reason to be confirmed on the corpus, finding T7) |
| Clock correction | 10 log entries, SE ≤ 20 ppm | Card clock, caveat | `clock::MIN_POINTS`, `clock::MAX_SD_PPM` | Count: not recorded (finding T8). 20 ppm: about the crystal error that leaving the correction out leaves in (its doc) |
| Power Reserve stop | 600 s with no run of 8 good beats | Run goes on | `power_reserve.rs` | Not recorded (finding T8) |
| Power Reserve hour | a whole hour, span ≥ 3,599 s | Labelled as the last part hour | `power_reserve::Hour::whole` | A second's slack for the sums that make it |
| Poise wave shown | 5 positions; direction only when its size clears 2 SE | "Fitted once 5 positions are measured"; no direction | `poise::MIN_SHOWN`, `poise::CLEAR_SE` | 3 fix the wave, 2 more check it; a direction inside the noise is noise |
| Heavy spot | every amplitude outside 219.5° ± (2 SE + 8%) | Withheld, chip | `poise::NULL_AMPLITUDE_DEG`, `NULL_BAND_SHARE` | The poise effect is near zero there; 8% covers a catalogue lift error (one measured: 7.3%) |
| Noise lead left out | 20 s of watch after it | Nothing left out, no watch | `session::LEAD_WATCH_MIN_S` | Two whole scatter windows (its doc) |
| Unlock coverage listed in findings | 60% | Finding listed | `session::UNLOCK_COVERAGE_LISTED` (finding T2) | Must be `MIN_WINDOW_COVERAGE` |
| Measurement time listed in findings | 40 s | Finding listed | `session::SHORT_MEASURE_S` | Witschi's "≥40 s measurement time" (D40e, 2025) |
| Period search, a line on the beat timing | a rate swing of 0.05 s/d, peak to peak, from the line's fitted shape | Line not named | `longterm::MIN_RATE_SWING_S_PER_DAY` | Half the rate's 0.1 s/d place (section 11): a swing that rounds to nothing is not a figure. A share of the variance can't serve, because the timing is a walk whose power rises as 1/f² towards long periods: on YM42's 2 h take 65% of the detrended power lies past 36 min and the 5 s escape wheel's line holds 0.006% of it |
| Period search, a line on a reading series (amplitude, beat error) | 0.5% of the detrended variance | Line not named | `LongConfig::min_explained` | Readings are near white under the detrend, so a share of the variance is a size; the number is not recorded beyond that (finding T8) |
| Period search, longest period on the timing | min(span/3, 1.2 × the detrend window) | Not searched | `longterm::SLOPED_REACH` | Where the two-pass detrend keeps half a sine: measured on 6 h at 0.5 s, with and without a ±40 s/d ramp, noise 0 and 0.3, it keeps 1.00 up to 0.5 W, 1.4 to 1.76 near 0.7 W (the plain median 1.3 to 1.4), 0.92 at W, 0.53 to 0.56 at 1.2 W and 0.36 to 0.39 at 1.33 W. The search and its null use the same reach |
| Period search, detrend across a break | no detrend window crosses a break (a stop, a restart, a cut) | Each piece detrended alone with the take's window; a piece shorter than one window, and the timing bin a break falls in, left out | `longterm::detrend_pieces`, `longterm::piece_starts` | A break re-fits each piece's timing, so the series jumps by up to seconds there; a window across it smears the jump into spikes that swamp every line (YM42's 59 h take: 117 breaks after 55 h, 162 ms rms) |

- **Breach (decided 29 Oct):** no rule yet ties a reading to the share of its windows that were usable; see Decisions. #645. (The 10% and 60% floors above now cover window figures.)

### Audit, 13 Nov: thresholds to fix

Each item below breaks rule 1, 2 or 4. Each gets an issue. None changes a figure except T1.

- **T1. Walk 50% (`walk::walk`, `2 * r.len() < per_bin`).** A literal that differs from rule 2. Fixed to 10%/60% in #1496.
- **T2. `session::UNLOCK_COVERAGE_LISTED` 0.6** restates `confidence::MIN_WINDOW_COVERAGE`. Use the one constant.
- **T3. "30 readings" written seven times as a literal** (`steadiness_beat_error.rs`, `amplitude_slope.rs` twice, `analysis.rs` period search, `steady_views.rs` beat error search twice: in `searches` and in the views builder, and `steadiness.rs`'s structure search with its 120 s, also a literal) beside `MIN_PAIRS` and `joined::MIN_S`. Make it one named constant with `MIN_PAIRS`'s reason, or name each with its own reason.
- **T4. The app's strip rate line needs 6 beats** (`strip.rs` `RateLine`), restating the timing fit's 6 in `timing.rs` `solve`. Name the fit's minimum in core and use it in both.
- **T5. The anchor's 16 beats** are written in the app (`take_io.rs`) and the CLI (`record_cmd.rs`). Use one core constant.
- **T6. The app's "no watch yet" waits 6 s** (`readouts.rs` `no_watch_yet`), a literal in the app. Move it to core with its reason.
- **T7. Unnamed method minima:** histogram 3 and 4, a raster quantile 4, steadiness 4 and 10, diagnose 8, shape 10, the long-run slow-out 4. These are method minima, but they need names and reasons. The wheel-lines 90% needs its reason confirmed on the corpus.
- **T8. Thresholds with no recorded reason**, marked in the table: the timing fit's 6 beats, the 3 beats per side, 60% coverage, 95% beats found, the isochronism bin's 10, the clock's 10 entries, and the Power Reserve's 600 s and 8 beats. Each needs a reason from the method or a corpus check that the number is not arbitrary in effect, recorded here. Until then they stand, because changing one moves figures.

---

## 8. Medians and means

| Figure | Statistic | Why |
|---|---|---|
| Rate | Least-squares fit over beats, robust to outliers | Uses every beat, cancels beat error (Witschi) |
| Rate over several stretches | Mean of fits, weighted by beats used | The rate over the time heard |
| Amplitude, beat error (unlock) | Median of 2 s windows | A mis-marked edge moves one window, not the figure |
| Window amplitude | Mean of tick and tock | One number per window |
| Positions Test per position, X | Mean | As NIHS 93-10 defines the positions values |
| Steadiness bins | Median of fine windows (#656) | The core reading's statistic, so Steadiness and the charts agree. Every window is required, since the Steadiness tests are calibrated on readings of equal span (#1364) |
| Long-run grid | Median per step | Period search on a robust series |
| Period search, a found cycle's size and what is taken out | Least-squares sines at the cycle's kept harmonics, fitted to the grid, read at each phase bin's middle; the fold's medians only for the raster | One fit sizes the cycle and takes it out, and the removal also takes the cycle's further harmonics that stand out of the spectrum (`standing_harmonics`, as the misfit does), short of a wheel's: a pulse of 8 s every minute left its 9th harmonic (6.7 s) to be named as a cycle of its own. On DD3235's 30 min take the 30 s line reads 3.11 s/d; a least-squares fit of its fundamental to the 0.25 s grid the search reads gives 3.15, and the median fold gave 2.97. A fit over the raw beats gives 2.65 with the app's jump cut (quality > 0.4) and 6.11 without it: 127 jump beats carry most of the raw line |
| Period search, a harmonic of a found cycle at a train wheel's period | Left out of the cycle's shape, size and removal; the wheel's own line stays for the next search | A harmonic within the wheel tolerance of a wheel (`wheel_of`, period²/span) is that wheel's line: DD3235's 30 s cycle carried the 5 s escape wheel as its 6th harmonic and read 20 s/d with it (Norm); left out, it reads 3.11. A line at a wheel's period is that wheel's line, never a harmonic of another found cycle, so DD3235's 5 s stays named and listed. With no train, nothing is left out. The wheel lines (`wheel_lines::lines_for_take`) read the timing with the plain running median by piece, not the search's sloped pass: its gain of 1.4 to 1.76 near 0.7 of the window raised the wander inside each wheel window 1.3× and doubled the fourth wheel's standard error on YM42's 2 h take (0.052 to 0.100 ms). A harmonic shows in the shape only past the 1% χ² bar of the noise read with all 8 harmonics taken out, so a line at another harmonic (a 40 s line at a 240 s cycle's 6th) is not read as noise (Norm, 13 Nov) |
| Period search, the steady pane's false alarms at 30 min, ρ 0.7 | At most 15 of 300 seeded takes name something (`a_steady_watch_with_red_noise_names_little_on_the_whole_pane`) | Peter raised the cap from 13 to the pane's 5% target on 13 Nov ("Raise it to 15 (5% target)", #1601): with the sloped timing detrend the cell names 14 on its seeds, the extra being chance first lines on a slow wander, and 12 and 10 on fresh seeds. No other cap moved |
| Spread of residuals, jitter | 1.4826 × MAD | A robust standard deviation (`dsp.rs:276`) |

- **One median.** `dsp::median` (dsp.rs:220): the middle value, or the mean of the middle two. `twostate.rs:166` keeps its own copy (same result), and `session`, `analysis` and `twostate` each wrap it again. (#512)
- **The median and the upper middle value (#838, #839).** A figure a user sees takes the median: `dsp::median`, or `dsp::median_f32` for f32 values. So do the envelope levels read from a run of samples or peaks: a beat's floor before its drop, the typical peak in `detect_after`, the per-sample side-sound levels, the amplitude edges' floor, the profile's floor and the shape's stretch levels (#839). Two levels take `upper_median_f32`, the upper of the two middle values, never an average of two. The burst gate takes it because it gates on a level a block actually reaches. The beats' median template (`beats::median_window`) keeps it because the median there reads a brief watch after a stand noise as a watch over the whole file; that trade is #844's. These are two named statistics, not two copies of one.
- **One percentile (#866).** The three nearest-rank copies (`analysis.rs`, `session.rs`, `longrun.rs`) and the three interpolated ones (`histogram.rs`, `distribution.rs`, `steadiness.rs`) now call `dsp::quantile` or `dsp::percentile`. Only p05 and p95 moved; no headline figure moved. The moves are largest where there are few values, where nearest rank and interpolation differ most: amplitude p05 of `long` on edge_quiet_step_20db, a take with no amplitude periods, went from 208.94° to 233.59° in the #866 review's run over 33 takes, and #866's own run found up to 1.53 s/d of rate on a 12-reading take.

---

## 9. Uncertainty

**Rule:** every ± is `confidence::standard_error` of the values in time order. Scatter from successive differences (1.4826 MAD / √2), lag-one correlation off a fitted line, effective count n(1 − ρ)/(1 + ρ), then 1.2533 s / √(1 − ρ) / √n_eff. Full model: `docs/research/measurement-precision.md`.

**Lives in:** `confidence.rs:129`. Rate uses every other 10 s window today (`confidence.rs:144`); decided (R1, #636), the ± is read from the tiling of 2 s cells. Live scales it to Average Over (`measure.rs:415`).

- **Enough values (decided 12 Nov, #1470):** the scatter and ρ behind a ± are read from at least 30 values (`confidence::MIN_NOISE_VALUES`; `confidence::standard_error_borrowing`). A reading with fewer, such as a live 10 s average of five 2 s windows, takes them from the 30 values nearest it in the same take (`confidence::noise_pool`): the latest ending where it ends, and for a saved reading early in its take, the next after it. They are read across cuts, masks and positions, each stretch on its own, so no difference spans a cut; never across takes. n, n_eff and the between-stretches term are the reading's own. The rate counts every other 10 s window, so its ± needs about 300 s of beats. The values are borrowed from no further than `session::BORROW_LOOK_S` (an hour) either side, since noise from further away describes a different state of the watch (section 7). With fewer than 30 within an hour either side there is no ±. The figure itself is still read from the reading's own values.
- **ρ corrected for its bias (#1544):** every ± reads ρ through `confidence::corrected_lag_one`: the pooled lag-one correlation plus Marriott and Pope's (1954) 2(1 + 3ρ)/n for each stretch's line taken out, n the stretches' mean length, the same term `long_run_rho` adds. Read from 30 values the raw estimate falls 0.07, 0.15 and 0.26 short of a true ρ of 0, 0.5 and 0.9 (formula 0.07, 0.17, 0.25), so the ± under-covered at high ρ: 48.4% at ρ 0.9 (seed 20261114). Corrected, a borrowed ± (5 values, noise from 30) holds 74%, 81% and 75% at ρ 0, 0.5 and 0.9 on a fresh seed, and 72% on 20261114. ρ is held to `confidence::MAX_RHO` (0.99): past it the inflation √((1 + ρ)/(1 − ρ)) cannot be read from about 30 values, so the ± is left wide. At ρ 0.9 and n 30 the corrected ρ reaches the limit about 21% of the time. The ± of wheel lines no longer reads ρ: it reads the residual's own power beside the line (#1618, below); the coherence noise level (`timefreq`) does not read it yet.
- **What it is:** an estimate of precision from the take itself, under an AR(1) noise model. It is not accuracy: lift angle and clock are caveats, not part of the ±.
- **Coverage test:** a seeded test holds coverage between 55% and 85% at ρ 0, 0.5 and 0.9 (`standard_error_allows_for_correlated_readings`).
- **Breach:** where the beat error turns over, the figure is a median of sizes, but its ± is taken from the signed values, so the turn widens the ±. Finding N1, #637.
- **Gapped takes (#639, #1637):** a ± reads its values in stretches (`confidence::stretches`). A new stretch starts where a window starts after the last one ended (a cut, a mask, a window left out) and after a window that read nothing, since the values either side of it are lag two apart, not neighbours. For windows that tile, as every caller's do (amplitude and beat error 2 s windows; the rate's every other 10 s window), a window kept with no value and one left out give the same stretches. Where windows overlap, `a > end` can't see a window left out, so only a kept empty window cuts there. No difference is taken across a gap, ρ is pooled over the stretches (`confidence::pooled_lag_one`), and stretches at different levels add the between-stretches term. Before #1637 a window with no value was passed over without ending its stretch, so the values either side were read as neighbours. On the audit's case (30 windows at 270°, 100 empty, 30 at 250°) the ± read 0.0° with the empty windows kept and 12.5° with them left out. Both now read 12.5°.
- **Wheel lines (#832):** a bin's size is its windows' sizes weighted by 1/se², and its ± allows for windows that overlap: two windows sharing a fraction f of their (equal) length are taken to correlate at f, as two fits on white noise do. Seeded, 300 takes: ±1 SE holds 67% (nominal 68%) with scatter over SE 1.01; with a 30 s hole through the bin's windows 67% and 1.00, against 54% with the windows taken as independent. With no line in the bin, the sizes are expected to correlate less than f, so the ± should err wide; this is reasoned, not tested. Each window's own SE is read from the residual's power at the frequencies one to six whole cycles a window either side of the line (`wheel_lines::near_power`), not from its scatter and a lag-one ρ̂ (#1618): on the 0.25 s timing grid the residual wanders like a walk, ρ̂ sat at the 0.99 clamp in 10 of 11 windows on YM42's 2 h take, and the AR(1) power that implied at the line's period set the ±. At 14 periods no wheel turns at, a window's size over its SE should have an rms of √2 (a Rayleigh's). On YM42 DU 2 h the timing read 0.59 with the clamped ρ̂ (its ± 2.4 times too wide) and reads 1.32 from the nearby power; the amplitude 1.21 and 1.40, the beat error 1.42 and 1.42; on YM42 CU 8 h the timing 0.58 and 1.22, on the Reverso 2 h 1.38 and 1.24. Seeded, a walk of 0.02 ms a step under 0.05 ms of jitter on a 0.25 s grid: 0.67 before, 1.39 now. On a red spectrum the neighbours' mean errs high, so the ± errs wide (the timing reads 1.22 to 1.32, under 1.41).
- **Steadiness:** the CUSUM assumes independent readings (`steadiness.rs:991`, `cusum.rs:11`). Decided: the step-robust long-run σ (#647), and the same for the change finder (#649).

---

## 10. Clock rules

**Rule:** one clock per take, picked once in the order of section 3. Every reading is made on it.

**Lives in:** `take::clock_from` (take.rs:51). The fit maps each beat onto true time before any reading (`clean.rs:187`).

- **A clock log is applied** only from 10 entries with an SE of 20 ppm (1.7 s/d) or less (`clock.rs:169-176`).
- **Sources disagree** when the USB lines and the log are 5 ppm (0.43 s/d) and 2 SE apart (`clock.rs:160`). With no clock log, the USB lines are weighed against a stated or kept correction instead: the caveat fires when the two are more than 5 ppm apart, with no SE test, since a correction carries no scatter of its own (decided 29 Oct, #635). #635 brings the code to the order in section 3.
- **ppm to s/d:** 1 ppm is 0.0864 s/d (`clock::rate_error_s_per_day`). A rate is corrected exactly, (1 + r/86400)/(1 − ppm·10⁻⁶) (`clockstore::correct_rate`); a card `ppm` fast makes rates read slow by ppm × 0.0864 s/d.
- **Live** corrects the rate by the mean ppm over the span, after the fit (`live.rs:961`). For a straight-line clock this matches mapping each beat.
- **Breach:** Power Reserve hours are cut every 3,600 s of sound-card time, not true time (`power_reserve.rs:255`, #642).

---

## 11. Rounding and units

**Rule (Peter, 12 Nov; built in #1470):** every figure in text is written to its place below and never finer, whatever its ± (decided over "one place finer" and "as the ± allows"). The ± measures repeatability only; the lift angle and the sound card's clock limit accuracy to about these places, so more decimals would claim a precision the reading of the watch doesn't have.

| Figure | Unit | Place | Sign |
|---|---|---|---|
| Rate | s/d, + gains | 0.1 s/d | Always; zero has none |
| Rate p05 to p95 | s/d | 1 s/d | Always |
| Amplitude | degrees | 1° | None |
| Beat error | ms | 0.01 ms | Size shown; signed in JSON (Tick late is +) |
| Jitter | µs | 1 µs | None |
| Clock | ppm, + card fast (#657) | 0.1 ppm | Always |
| Slope against amplitude | s/d per degree | 0.01 | Always |

- **The ±** is written at the figure's place, rounded up, and never below one unit of the place: a 0.3° ± on an amplitude reads "149° ± 1°", a 0.04 s/d ± on a rate "+11.4 ± 0.1 s/d". Rounding an uncertainty up is the safe direction. The exact ± and the values it came from go in the hover ("± 0.3° from 5 windows; shown to the nearest 1°").
- **A spread written after ±** (Steadiness's "± from one reading to the next", the Allan plot's least scatter) follows the ± rule: at the figure's place, rounded up (Norm, 13 Nov, #1470). Plot axis ticks are a scale, not figures, and keep 3 significant figures.
- **A difference within one take** (a trend per hour, a cycle's peak to peak) is written one place finer than its figure: 0.01 s/d (per hour), 0.1°, 0.001 ms (`figure_text::DIFFERENCE_EXTRA_DIGITS`, `difference_digits`). Both ends share the take's lift angle and sound card clock, so the clock cancels and the lift angle only scales the difference by a few percent: repeatability sets its limit, not the accuracy that sets the places above (Norm, 13 Nov, #1470).
- **A value worked out from figures (Peter, 13 Nov, #1576):** "we should apply normal sig-fig rules where we know the inputs to the calculation to a certain number of sig-figs, that number of sig-figs should flow through to the derived values." A value calculated in closed form from figures with a place above keeps as many significant figures as the fewest among those inputs, as written at their places. For example, the beat error angle comes from the beat error and the amplitude, so 0.46 ms at 302° gives 1.7°, and 0.47 ms at 129° gives 0.76°. Its ± is rounded up at that place. Three limits, which are Norm's reading of the scope:
  - A result that is itself a figure in the table keeps the table's place, because that place is the accuracy limit. A rate read on a clock in ppm, for instance, is still written at 0.1 s/d.
  - A fit over many readings (a slope, a trend, a cycle's size, the poise wave) is a statistic, not a calculation from rounded inputs. It keeps the places above and the difference rule.
  - A difference of figures (a complication's On less Off, Im, a position's change) is a calculation like any other. It keeps its inputs' place. The one-place-finer rule above is only for fitted trends and cycle sizes.
- **No ±:** a figure without one (too few values, section 9) is written alone at its place, with no "± —"; the hover says why.
- **Cards:** the place is fixed, so the value's width never changes as its ± moves, and the figures are tabular (Hugh, 12 Nov).
- **One formatter (decided 12 Nov, built in #1470):** core's `figure_text` (`base_digits`, `figure_with_pm`, `pm_text`) writes the value, its ± and the place for every surface (cards, tables, strip labels, the CLI, MCP and reports). Nothing else rounds a figure for text, and a surface that needs a place names a `base_digits` constant instead of typing one.
- **Not rounded:** JSON is never rounded. CSV keeps full precision, as data. Charts and sparklines plot full precision; the rule is about text.
- **Rate:** (P_nominal / P − 1) × 86,400, exact, with P the fitted beat period (`timing.rs:163`).
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
| Isochronism | Rate median per 2° amplitude bin, weighted line (no quadratic since 13 Nov: no source as a method, and its turn came from the fit's form) | **Partly** Noise in each amplitude reading shrinks the slope toward zero, and anything else that changes with time (temperature, position) is credited to amplitude. True −0.5 s/d per degree reads −0.11 on 30 min with a 3° fall, −0.29 on 2 h with 6°, −0.45 on 44 h with 15° (1.5° reading noise assumed). No ± | Run, #651 |
| Isochronism walk | Median rate of the 10 s readings and median amplitude of the 2 s windows per 5 min, each series read on its own; each series withheld under 10% of the stretch's readings or windows (`confidence::MIN_WINDOW_SHARE`) and thin under 60% (`MIN_WINDOW_COVERAGE`), as every window figure is (#1429), and a stretch with either withheld is left out, never given an x; a thin point is drawn faint, its hover giving the counts, and a merged point is thin when any of its stretches is; merged to a surface's width by means of the medians, each series weighted by its own count (`readings` is the rate's 10 s readings, `amp_windows` the amplitude's windows) | **Yes** A picture of the data: medians resist the odd bad reading, and no slope, fit or ± is read from it. A merged point is a mean of medians, not the median of its readings. Pairing each reading's rate with its amplitude needed all five of its windows, and with 82% of windows read that kept 37% of readings and 2 of 6 bins on a 30 min take | Read, #741; floors Norm 11-12 |
| Spectrum (FFT) | Hann-windowed FFT of the whole grid | **Partly** Empty readings are set to the mean, so line heights fall with the share missing. Welch, beside it, uses only unbroken stretches | Read, #652 |
| Period search | Spectrum divided by the AR(1) noise shape (ρ by `long_run_rho`, as everywhere) and, under the running median, by the median's own noise shape for periods under its window; then levelled by a local median; harmonics summed; false alarm corrected for the number of trials | **Yes** Seeded red noise (ρ 0 to 0.9, 1 min to 2 h) names a cycle in 0 to 11 of 100 takes per cell, against up to 35 before; the steady pane names something on 8 of 200 control takes. A sine on 1 min is found less often (85 against 136 of 1000), as the sine lifts the ρ read from 60 points. Under 2 h, a cycle named on the amplitude or the beat error at ρ ≥ 0.7 carries `period_may_be_noise`: a slow wander under white noise is not AR(1), and 30 min nulls named one in 4 to 10% of takes (9.2% in the case #916 reports); decided 4 Nov, no figure changes. Under the running median, the level for periods shorter than its window is read only from periods shorter than the window: 30 min nulls now name a cycle in 2.2 to 9.2% of takes (9.2% to 8.0% in the #916 case), pooled 3.96% to 2.86% of 14,000; a 300 s cycle in 30 min is found in 49 of 100, and its test's bar was lowered from 75 to 49, decided 4 Nov. Since #1195 the search is held to a simulated null of its own pipeline (readings AR(1) by ρ; timing a random walk plus white jitter by its crossover; per take length and step): the null's whitened mean is divided out, each bin scored by its own null spread (a gamma shape per frequency), and the maximum score read against its fitted extreme-value tail. Timing's mix is a nuisance the take can't identify, so its tail is the least favourable over the true mixes that read as each fitted mix (and its neighbours, which the lookup blends), in location, scale and shape. On fresh seeds (200 a cell) every model, mix group and single mix stays at or under its upper 99% bound at p 0.01, 0.001 and 0.0003: readings 402 and 40 against 432 and 43 expected, timing 320 and 27 against 420 and 42; timing mix 3 and readings ρ 0.95, marginally over in one batch, held in a second of 400 a cell. On long takes (13 Nov) the timing is detrended in two passes, a local robust line and then the running median (`Slow::Sloped`): the plain median over a sloping timing returned the middle sample and took the cycles with the slope, so the rate named nothing on YM42's 40 to 59 h take. No detrend window crosses a break. Each found cycle is taken out as least-squares sines at its kept harmonics (`without_cycle`, shared with the misfit), not as its fold: a staircase of median per phase bin left a sawtooth at the bin rate on a steeply wandering timing, and the next search named its harmonics (11.8 s at 16 after a 306.6 s wander on 30 min of AR(1) rate). Later lines are scored on the levelling the first search read. The timing table's means and shapes, like its tails, are read by fitted mix, least favourable over the drawn mixes read as each cell: the fit reads the mix only at x ≥ 2, so below it, where the sloped detrend's response rises to 1.4 to 1.76, a walk read as white was levelled by white draws, and mix 3 passed p 0.001 2.9 times in 100 on 30 min. The table is now conservative: on fresh seeds (200 a cell) timing reads 313 and 27 against 442 and 44 expected, every drawn mix within its upper 99% bounds at 0.01, 0.001 and 0.0003, at some cost in power at long periods (Norm). Past p 0.0003 (`longterm::SIMULATED_P`) a false alarm is the fitted tail carried past its 400 draws a cell (timing's ξ 0.02 to 0.04 gives the 5 s line on the Reverso's overnight take 10⁻⁸⁷), so the text says the line is beyond the simulated range and the JSON keeps the number with `beyond_simulated` (#1243); the plain trials count's tail, where the table doesn't reach, keeps its number. On 300 white-noise takes of 30 min the pane names something on 18 (31 before), 11 of them Ljung–Box, which #1199 brought to 4; seeded red noise names 0 to 20 of 100 per cell, 30 min white and AR(1) nulls cross 1 in 100 and a slow wander under white noise 1 in 27. A rate cycle on timing whose band powers, with the named cycles taken out as least-squares sinusoids (the harmonics the search kept, and any further harmonic standing past the level noise passes once in 100 over those tried, `EXTRA_LINE_P`: a sharp pulse's harmonics read 0.32 against 0.28 on a synthetic take without it, 0.12 with it) with the bands at the kept harmonics left out and, for the further ones, only the bins beside each line, sit further from the fitted walk plus white than the null's 99th percentile for its cell (2,000 draws a drawn mix, read at the residual's own mix) carries `timing_model_misfit` (#1219: flicker is not modelled); on fresh takes with a sine added it is given on 251 of 41,780 (0.60%), every cell at or under its upper 99% bound (one at 7 of 200 held in a second batch, 5 of 400); at 300 s and 600 s the limit reaches 0.86 and 0.98 at mix 300, so there it seldom fires; flicker timing at 5400 s passes it in 20 of 20, and flicker under a sharp pulse still does; on Peter's corpus only YM42 CL 90 min does (0.26 against 0.24) | Run, #755, #761, #916, #1030, #1195, #1219 |
| Cross-correlation | Rate against amplitude after a 30 min trend is taken off; n_eff from ρ_xρ_y | **Partly** The test allows for correlated series, but its slope shrinks with amplitude noise as the isochronism slope does | Read, #651 |
| Allan | Overlapping Allan deviation, full blocks | **Yes** Made for clock noise | Read |
| Two states | Two-normal mixture on 10 s block medians after a ±300 s running median, with separation, switching and share guards | **Yes** The fit assumes independent blocks, but the guards stop a correlated wander passing | Read |
| Distributions | Freedman–Diaconis bins, peaks over 3 bins, normal line from IQR/1.349 | **Yes** No false second peak in any run on one-peaked noise, ρ 0 or 0.9, 180 to 3,600 readings | Run |
| Wind-down | Median of a 30 min stretch at the start and at 24 h | **Yes** Medians resist the odd bad reading; the change has no ± | Read |
| STFT | 20 min Hann windows stepped 5 min, line taken off, only windows with every reading present | **Yes** Nothing filled in | Read |
| Wavelet (SST) | Morlet transform on each unbroken stretch, edge zone (cone of influence) left empty | **Yes** Nothing filled in across a gap | Read |
| Wavelet coherence | Read against the 95th percentile of red-noise pairs with each series' own ρ (Grinsted 2004), fixed seed | **Yes** Correlated noise is the reference | Read |
| Kurtogram | Spectral kurtosis over half-overlapping Hann windows; noise band 2/√windows | **Yes** Noise reads −2/windows rather than 0 (−0.23 at 8 windows, −0.03 at 64), against −1 for a steady line; the stated band is wider than the real spread, so it errs safe. Same at ρ 0 and 0.9 | Run |
| Long runs | Residuals of the whole-take fit, median per grid step on watch time (beat number × nominal period), then the period search | **Yes** On watch time a wheel's period is exact even as the rate drifts. Its percentile is core's `dsp::percentile` (#866) | Read |
| Power Reserve | Ends after 600 s with no run of 8 good beats; each hour measured like a take | **Yes** It times the last beat heard, which near the end of the reserve may be a little before the last tick. Hours cut on sound-card seconds (#642) | Read |
| Positions Test | Means per position, NIHS 93-10 indices | **Yes** The standard's own method | Read |
| Dynamic poise | Weighted least squares (1/SE² of each position's rate) of the upright positions' rates on a constant, cos α and sin α, α the hour at the top: the measured wave (size, fastest angle), a fact. The heavy spot is inferred by the same fit with f(θ_i) = 2·J1(θ_i)/θ_i of each position's own amplitude on the cos and sin terms; withheld when any position's [θ − 2 SE, θ + 2 SE] widened by `poise::NULL_BAND_SHARE` (0.08) of θ holds 219.5° (`poise::NULL_AMPLITUDE_DEG`). cos 2α and sin 2α fitted beside as a residual check. No fastest hour or heavy spot is named unless the wave's size clears zero by 2 SE (`poise::CLEAR_SE`), and none without a ±. A known isochronism slope s is taken off as s·(θ_i − mean θ) before the heavy spot; its ± is common to every position, so σ_s²·g·gᵀ is added to the fit's covariance, g the fit's response to θ_i − mean θ; the amplitude's own noise it brings in, s·SE_θi, is separate at each position and widens that rate's ±. f(θ̂) carries the amplitude's noise into the regressor too: Û·f′(θ_i)·SE_θi·cos(α_i − ψ̂) is added to each position's ± from a first fit, then fitted once more. The angle's ± is by test inversion, asin(min(1, 1.96·σ⊥/size))/1.96, σ⊥ the size's ± across the fitted direction: the delta method at high signal, wider at low. With no slope, an amplitude once-round wave clear of zero by 2 SE is a caveat on the measured wave, stated in the null band too | **Yes** Each position's ± already allows for correlated readings within it, and the positions are separate stretches. On fresh seeds 3061–3069 (900 tests a case, run once with nothing changed after) the ± of the wave's size and angle, and of its cos and sin components with no wave, holds the truth in 92 to 97.5% at three, four and eight positions; the heavy spot's angle likewise at 180° and 260° (4 and 25 SE), and with a known slope at 4 and 3 SE of the full ± (rate, slope and f′ terms), stated in 0.996 and 0.926 of tests; withheld at 225°. Run on a real take: the corpus's four YM42 verticals (ym42_CU_8h, _CL_90min, _CD_90min, _CR_long, part 002 each, lift 55°) give a wave of 44.4 ± 1.3 s/d fastest at 217 ± 1°, an amplitude wave of 6.7 ± 0.5°, and the heavy spot withheld at 224–237°; the four takes were at different states of wind | Run, #1474 |

Fixes decided 29 Oct: see Decisions (change finder #649, autocorrelation #650, isochronism and cross-correlation #651, FFT #652).

---

## 13. Findings

Confirmed problems are filed as issues, and the fixes go to each file's owner. None is fixed by this guide.

| # | Finding | Where | Status |
|---|---|---|---|
| N1 | Beat error ± from signed values when the figure is a median of sizes | `measure.rs:498`, `session.rs:579` | **#637** |
| N2 | Saved fit did not cut at a pause over 2 s between beats | `session::measure_masked`, `Cuts::fit_cuts` | Fixed, #638 |
| N3 | ± on a gapped take takes differences across gaps; ρ not pooled per stretch | `confidence.rs:144` | **#639** |
| N4 | No core coverage rule; the 60% unlock share lives in `regress` | `regress_cmd.rs:32` | **#645** |
| N5 | Synthetic rate is first-order: +0.12 s/d at +100 s/d | `synth.rs:171` | Fixed, #816 |
| N6 | A window's amplitude fell back to one side with no caveat; tick and tock can differ by degrees | `amplitude.rs:227` | Fixed, #860 |
| N7 | `confidence::beat_error_se` is used only by its test; `assess` repeats it inline | `confidence.rs:155` | Fixed, #822 |
| N8 | CUSUM assumes independent readings | `steadiness.rs:991` | **#647** |
| N9 | The paper strip places beats on wall time (no clock correction) and draws the Rate line at the corrected rate, so on a corrected take the two differ in slope by 0.0864 s/d per ppm (confirmed by building and running). One clock everywhere decides the fix: the dots move onto true time, in core. | `timing.rs:63`, app `strip.rs:200` | **#643** |
| N10 | A listed pause the clock log saw no step for still adds its gap to true time | `clean.rs` `Cuts::true_time` | Fixed, #829 |
| M1 | Change finder assumes independent readings | `steadiness.rs:1040` | **#649** |
| M2 | Autocorrelation and Ljung–Box take off the mean only, so a trend reads as memory | `steadiness.rs:946` | **#650** |
| M3 | Isochronism slope shrinks with amplitude reading noise, is confounded with time, has no ± | `steadiness.rs:2678` | **#651** |
| M4 | FFT sets empty readings to the mean; heights fall with the share missing | `spectrum.rs:218` | **#652** |
| R2 | Steadiness makes its own readings (mean of fine windows), not the core reading | `steadiness.rs:2515` | **#656** |
| R5 | Clock sign is + when the card runs slow, against the usual convention | `clock.rs:29` | **#657** |
| R6 | Beat error falls back from the unlock to the drop with nothing saying so | `session.rs:413` | **#658** |
| K1 | Two percentiles; median copies | six files | Percentiles fixed, #866; #512 |
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

