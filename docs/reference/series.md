# Structure in time: the `series` command

A histogram or a probability plot shows whether a take's readings have
one hump or two, but not a cycle, a step or a slow wander, because it
throws the time order away. `timegrapher series` keeps the time order.
It reads the rate, the amplitude and the beat error of a take as three
series, runs the tests below on each, and states what they find with
the statistics behind it. It does not say whether a watch is good or
bad; that is for the person or the agent reading the facts.

Each series' headline leads with the first of these that the tests
detect, and adds a sentence for the others:

| Detected | Headline states |
|---|---|
| two levels | the two levels and how often the series switches between them (the two-state finder in `twostate.rs`) |
| two levels on one side only | the same, and which side carries them: "Tick windows fall in two clusters; Tock windows do not", or that the beat error from the unlock jumps between the levels while the one from the drop does not |
| a cycle | its period, the wheel whose turn it matches, and its size peak to peak. When the tests find a cycle but can't size one period, the headline still says so: "Amplitude drops to a low level and back on a cycle of about 78 s." when the two-level finder sees the dips ("of about" only when it times them), otherwise "Amplitude repeats on a cycle." |
| level changes | the number of changes and the level of each stretch; beyond three changes, the number, the range, and the first and last levels (every stretch is in `changes.segments[]`, and the text output lists each with its start and end) |
| a trend | the start and end level and the change per hour |
| correlation between readings | the lag-1 correlation, the Ljung–Box p, how long the correlation lasts, and the Allan excess |
| none of these | the number of readings, the median and the reading-to-reading scatter |
| a period search line faster than the readings | the period, the wheel whose turn it matches, the size peak to peak and that the readings are too coarse to show it, e.g. "the beat timing repeats every 5.0 s, the escape wheel's period, 0.240 ms peak to peak; 10 s readings are too coarse to show it". The "none of these" and correlation headlines carry it in place of "no cycle"; the cycle, two levels, level change and trend headlines add it as a sentence of its own ("The beat timing also repeats every 5.0 s, …" beside a slower cycle). For the rate a line under 30 s is the beat timing's swing in ms, and a slower one (with `--reading 20` or longer) the rate's swing in s/d; "faster than the readings" means under three of the readings cycles are looked for in: the 2 s windows for the amplitude and beat error, the readings for the rate |
| too few readings | N readings over N s (it needs 30 over 120 s) |

After these, the headline names the period search's lines it has not
named yet, as a last sentence: "The amplitude's period search also finds
45 s (7.4° peak to peak)." Only the search's strongest line has a size;
the others are given by period, in seconds. A line counts as named when
it is within the resolution (the period squared over the take, at least
1%) of the cycle or the shorter cycle inside it, or when it is the fast line above. The
search lists only lines that pass its own significance test, so the
sentence adds no threshold. When it follows a lead that would say "no
cycle … detected", the lead leaves cycles out of that list.

The thresholds are detection thresholds, stated with each test below
(a false-alarm chance, a share of the variance explained); none is a
tolerance for a watch.

The p-gated tests across the three series, each one's period search
lines, its Ljung–Box test, its trend's F test and its CUSUM's test of
one level (#917), are one family: a line, memory, slide or level change
is named only when its own test and Holm's
correction at 0.05 across the family both keep it (#659), so one take's
several tests do not add up to a false alarm. A peak of the
autocorrelation carries no p-value, so in the pane it only refines a
cycle the period search kept. Each test's own figures are given as they
are. The target is a steady watch naming nothing on 95 takes in 100; on
the seeded 2 h control scenario it names nothing on 192 takes in 200
(#723; `a_steady_watch_names_nothing_on_the_whole_pane` in
`scenarios.rs`). On steady takes whose readings follow themselves
(AR(1)), the counts below are the caps kept in
`a_steady_watch_with_red_noise_names_little_on_the_whole_pane` and
`long_steady_watches_with_red_noise_name_little_on_the_whole_pane`
(`scenarios.rs`), set once the period search was calibrated (#1195) and
the 2 s tiling made one (#636). They may only fall, and the tests are
the source. On 30 min the pane names something other than the
correlation (at ρ 0, where nothing correlates, a wander counts too) on
20 of 300 takes at ρ 0, 0 and 1 of 100 at ρ 0.3 and 0.5,
13 of 300 at ρ 0.7 and 20 of 100 at ρ 0.9; on 2 h, on 3 of 300 at ρ 0
and at ρ 0.7, 2 and 3 of 100 at ρ 0.3 and 0.5, and 7 of 100 at ρ 0.9;
on 8 h and 24 h, on at most 1 of 100 at any ρ (#916, #917). Takes of
30 min at ρ 0 and 0.9, and 2 h at ρ 0.9, miss the target of 5 in 100.
Part of the 30 min ρ 0.9 count is a level change named alone. That is
a floor rather than a bias: a take whose readings follow themselves
that closely holds about 10 independent readings, too few to read their
correlation closely. `long` (its text,
report and summary) and a session's cycles list the same lines: core
decides the kept set once (`steadiness::hold_to_family`).

```sh
timegrapher series take/ --lift 55 --clock take/clocklog.txt
```

It reads the same input as `long`: one recording, several segments in
order, or a folder; a take's folder, or one sound file its `session.toml` lists, is read with that `session.toml`, each
entry from the end of its settling and the stretches joined, as the
app's Steadiness panes read a position (see [long-runs.md](long-runs.md)). `--clock` corrects for the sound card's clock. The
other options are the same as for `long`. `--reading` sets the length of
each rate reading, 10 s by default. The beat error is the one measured
from the unlock; where its sign turns over across a break, its readings
are sizes, given as positive, as the take's beat error is (see
[long-runs.md](long-runs.md)), and a `Conditions` line and the
`beat_error_as_size` caveat say so. Where Tick and Tock were named by the
beat count across a break, not told apart by their sound, a `Sides` line
says so. A 2 h take takes about 30 s, and a 16 h run about 4
minutes.

All three series are tested on readings of the same length, the rate's.
The amplitude and the beat error are measured in 2 s windows; the
windows that fall in each rate reading make one reading, their median,
as the session's figures take theirs (`session::window_figures`).
The slopes against the amplitude pair each rate reading with its own
mean of those windows instead, each window weighted as the rate's
least-squares line weights its span, 6u(1 − u) integrated over the window
(0.104, 0.248, 0.296, 0.248, 0.104 for five windows;
`amplitude_slope::rate_fit_weight`). A short event inside a reading then
moves the amplitude it is paired with as far as it moves the rate. A
median moves all the way once most windows are in the event, and a
plain mean moves less than the rate does, so either one biases the slope
by about 10% on a rub of 8 s in 10 (#951). The amplitude series and its
chart stay medians.
No reading is taken over part of its span: an amplitude or beat
error reading needs every 2 s window its span holds (a 2 s window that
reaches into a break's gap is left out, so a reading beside a gap is
too), and a rate reading
needs beats in each 2 s piece of it and must not reach into a break's
gap from either side, so a reading at the edge of a gap or a masked
stretch, or with a window missing, is left out rather than averaged
over what is left. The Allan deviation's averages need every reading of
their block for the same reason. Testing a 2 s series and a 10 s series
side by side would compare different things: a 2 s window is noisier,
but it also keeps wander that a 10 s reading averages away, so the same
wander can show as correlation in one series and not in the
other. The headlines name the reading length.

Readings stepped every 10 s cannot show a change faster than 20 s, and a
10 s average already flattens anything near it (it keeps 93% of a 48 s
cycle, 64% of a 20 s one, none of a 10 s one). So cycles are looked for
in the 2 s windows for the amplitude and the beat error: the folds and
the autocorrelation candidates below use them, and a cycle there can be
as short as 6 s. The rate's faster lines, such as the escape wheel's,
come from the period search on the beat timings and are reported in ms
(see [long-runs.md](long-runs.md)).

## The views

For each series:

- **Autocorrelation**: how closely a reading matches the one a lag
  later, read with the slow trend (the period search's 30-minute running
  median) taken out first, so a rate drifting as the mainspring runs down
  is not counted as memory (a quadratic on a take under 20 minutes).
  Each lag is given less the correlation that taking the trend out
  leaves in white readings of the same count and detrend (about −3/n at lag one
  after the quadratic, #828; about −0.7 over the window after the median),
  so independent readings read about 0 and stay inside ±1.96/√n at every
  lag. The Ljung–Box test combines the first 10 lags into one p-value,
  each lag weighed by the pairs it is read from, and reads it against
  what those white readings give, not the plain χ² (#1199; drawn offline
  into `data/white-null.json`). `memory_s`
  is the lag where the correlation falls below 1/e. `repeat_lag_s` is
  the first lag where the correlation rises back to a peak after falling,
  which means the readings repeat. Where gaps leave fewer pairs of
  readings at a lag than a full series would have, the test and the
  search for a repeat weigh each lag by its pairs, and leave out a lag
  read from under 30 pairs (`ljung_box_lags` counts the lags kept; with
  none, `p_value` is 1): sparse readings say nothing about a cycle.
- **Allan deviation**: the scatter of averages over τ, drawn beside the
  white line, which is what independent readings would give (the scatter
  at one reading divided by √(τ / reading)). For independent readings
  the two lines stay together. Wander and drift lift the curve at long τ,
  and a cycle of period P puts a bump near 0.37 P. `best_tau_s` is the
  averaging time with the smallest scatter, so the most repeatable
  reading.
- **CUSUM**: the running sum of each reading's distance from the mean,
  in units of σ√n, where σ is the readings' long-run scatter
  (`long_run_sd`: the reading-to-reading scatter allowing for their
  correlation, read once their largest step is out; that step is chosen
  to fit, so the correlation gets back 3.3/n for it,
  `confidence::STEP_CHOICE_BIAS`, #917). With no change of level, correlated or not, it stays
  within ±1.36 about 95% of the time. A step in the mean bends it into a
  V or a tent. The band is a guide: the corner is marked only where the
  headline names a level change (#697): the change finder below cuts the
  series, the CUSUM's p (the Kolmogorov tail of its largest excursion,
  `p_value`) passes 0.01 and Holm's correction (#917), the stretches
  explain at least a fifth of the variance, and the lead is not a slide. The corner is the cut nearest the CUSUM's largest
  excursion. A step that only the CUSUM shows, that explains under a
  fifth of the variance, or that a line explains at least as well goes
  unnamed, though the line still bends.
- **Change points**: 30 s block medians split into stretches of constant
  mean, at least 5 minutes each. A cut is placed only when it is strong
  evidence against one level: the gain must exceed 3 ln n, and never be
  less than 15. The penalty only places the cuts; whether the level
  changed is the CUSUM's test, above.
- **Trend**: a straight line and a quadratic. The quadratic also covers a
  rise followed by a fall. Its p-value is the F test of the quadratic
  against the mean on n(1 − ρ)/(1 + ρ) readings, ρ their lag-one
  correlation after their line, which comes to (1 − R²)^((n_eff − 3)/2):
  readings that follow themselves wander far enough for a quadratic to
  fit a fifth of them often (#659). A slide is named only when the trend
  explains at least 20% and its p clears the family below.
- **Period search**: the strongest component found by `long`'s search.
  It scores each period together with its harmonics against a
  false-alarm chance (see [long-runs.md](long-runs.md)).
- **Two states**: the two-state finder over 10 s blocks (see
  [sessions.md](sessions.md)).
- **Time and frequency** (with `--maps`): two maps of each series'
  lines against the time of the take, on one log period axis, so a line
  that comes and goes, or drifts, shows where and when it does. The
  **STFT** is the amplitude spectrum of each 20 min stretch, stepped
  5 min, every period read over the same window, from four readings up to
  a third of the window (400 s); each row reads the strongest frequency
  in its band (halfway, in log period, to the rows either side), so a line
  between two rows reads within 20% of its size. The **synchrosqueezed wavelet transform**
  (SST) is a Morlet wavelet transform (ω0 = 6) whose window grows with the
  period, with each value moved to the period its own phase turns at,
  which sharpens a line to near its true period; it reaches up to an hour
  or a quarter of the longest unbroken stretch, whichever is shorter.
  Both give the sine-wave size at each period (half its peak to peak), in
  the spectra's units. Under noise the SST spreads a line over
  neighbouring rows, so one row reads under the line's size: read the
  size from the STFT, and where and when from the SST. Neither reads
  across a gap: an STFT window with a missing reading is left out, and
  the wavelet transform runs on each unbroken stretch of at least 64
  readings alone, with the times too near a stretch's ends for a period's
  wavelet (its cone of influence) left empty.
- **Coherence** (with `--maps`): the wavelet coherence of two series read
  at the same times, from 0 to 1, against time and period on the SST's
  axis: how tightly the two move together at each period and time, and
  the phase between them, so which leads and by how much (the phase's
  share of 360 of a period). Two pairs: the rate against the amplitude,
  on the 10 s readings the correlation below uses, and Tick's amplitude
  against Tock's, on the 2 s windows, where a cycle both sides share
  shows in phase. Each transform is smoothed over time (a Gaussian as
  wide as the wavelet's scale) and over about 0.6 of an octave of periods
  (Torrence and Webster, 1999). Beside each period is the coherence that
  95% of the values from pairs of unrelated red-noise series stay under,
  with the two series' own lag-one correlations (Grinsted, Moore and
  Jevrejeva, 2004), from 40 pairs with a fixed seed: by chance, about one
  value in twenty lies above it. As for the SST, each stretch where both
  series are read is transformed alone, the smoothing reads nothing
  outside it, and the times within a period's cone of influence (√2
  scales) of a stretch's end are left empty.
- **Lomb–Scargle** (with `--maps`): the periodogram of each series'
  readings, each at its own time, with nothing filled in across a gap,
  so a long run with pauses gives one spectrum over the whole of it (the
  FFT and the period search fill each gap with the mean). It is the
  generalised periodogram (Zechmeister and Kürster, 2009), with a
  floating mean and each reading weighted by the scatter of the readings
  around it (from one reading to the next, the 30 either side in the
  same stretch), so a noisy stretch counts for less; the power is the
  share of the weighted variance a sine at that period explains. The
  readings are the tests' (10 s), with the slow change taken off as the
  period search takes it off, from four readings' period to a third of
  the span. Each of up to eight peaks (a weaker maximum within two
  resolution widths of a stronger one is its sidelobe and is left out)
  carries its false-alarm probability over the whole band, by Baluev's
  (2008) bound. That is the chance of a peak this tall from readings with
  no cycle that are independent from one to the next: readings that
  wander are not, so their peaks read far below it. Beside it is the
  spectral window, the periodogram of the reading times alone: gaps that
  recur put lines in it (a 5 min pause every 30 min puts 0.037 at
  30 min), and a peak within one resolution width of a window line of
  0.02 or more (`on_window_line`) may come from the gaps. Without gaps
  the window stays under 0.009 in the band.
- **Wheel lines** (sizes by amplitude in the plain JSON, every window with
  `--maps`): for each wheel the calibre names (with no calibre, none, unless
  `--escape-teeth` names the escape wheel), the size of a sine at its turn's
  period, fitted by least squares (with a level and a slope) in windows
  of ten turns, at least 20 min, stepped half a window, in the beat
  timing (ms), the amplitude and the beat error, each with its slow
  change taken off as the period search takes it off. Each window has its
  standard error (from the residual scatter, as if the readings were
  independent), the amplitude it was read at, and the sine's phase at
  the take's start: a line locked to the wheel keeps one phase from
  window to window. The windows are combined in 10° bins of amplitude,
  each weighted by one over its standard error squared. The bin's
  standard error allows for the windows' overlap: two windows sharing a
  fraction of their length are taken to correlate by that fraction, so
  half-overlapping neighbours count less than independent windows
  (#480). A line that only
  shows at low torque reads near zero at high amplitude and rises below
  some amplitude; with no line in it, a window reads a size near 1.25
  standard errors. The timing and amplitude are on the watch's own time
  (the beats counted, as `long` reads them), so a turn keeps one length
  however the rate moves.
- **Wheel turns** (with `--maps`): synchronous averaging. Every whole turn
  of each wheel, cut on the watch's own time from its zero, is averaged
  phase bin by phase bin (one bin per reading of the series, at most 60
  a turn), each turn less its own mean, over the take and over the turns
  read in each 20° of amplitude (bands with ten turns or more). Whatever
  is not locked to the wheel averages away; what is left is the wheel's
  own shape, so a bad tooth shows at one place in the turn, and a band
  where it grows shows at what torque. Each bin has its standard error,
  the spread between turns over the root of their count.

Both wheel views leave out a window or a turn with more than 5 s missing
or under 90% read, and a wheel whose turn is longer than half the period
search's detrending window (15 min), since taking off the slow change
takes off most of its line.

Across the series, the rate is correlated with the amplitude, each
reading against the same reading. There is no lag search: the rate
answers the amplitude at once, and on a shared cycle a lag search finds
the half-turn where the two anticorrelate. The test uses
the changes within half an hour, with each series' slow change taken
out: two series that both drift correlate whether or not one drives the
other, and a drift leaves too few independent readings to tell. The
p-value allows for each series' memory (an effective number of readings
from their lag-1 correlations). The slope in s/d
per degree, r and p are always given, with no threshold, and beside
them the whole-take slope and r, drift included.

A cycle is reported when one that holds up when the readings are
folded at its period. Candidates come from the two-state finder's
regular low level, the autocorrelation's repeat, every later
autocorrelation peak above three times its band (in the pane, only when
the period search kept a line: the peaks carry no p-value), and every
line of the period search, not only the strongest (a slow cycle can sit behind a
strong escape-wheel line). Each candidate's folded shape must explain at
least 10% of the detrended variance beyond what noise would give. When a
turn holds a whole number of readings (within a tenth of one) or more
than 30, the share and size are the fold's; otherwise they are those of
sines fitted at the period and each harmonic the readings resolve (two
readings or more a turn, up to the 8th), since a fold of whole bins at
4.5 readings a turn (a 45 s cycle in 10 s readings) puts readings of
different phases in one bin and reads 0.8 to 0.9 of a sine's size. Of the
candidates the one that explains most wins, and then its shortest whole
fraction (down to an eighth) that explains at least 85% as much is kept:
folding at five turns of a cycle explains as much as folding at one, and
on a lag grid of 10 s readings a 48 s cycle lines up best at 240 s.
Every candidate's period is refined by folding: over a take of T
seconds a period off by P²/T drifts a whole turn, so on a 16 h run a
48 s cycle has to be found to a few thousandths of a second. The search
narrows until its step is a twentieth of that. When the longer period
still wins, the cycle inside it is named if it explains at least half
as much on its own: on the Reverso Duo replica's overnight run the rate repeats
every 240 s as five turns of a 48 s cycle whose shape changes from turn
to turn. With the 48 s shape taken out, folding at 240 s still explains
6% of the rate's variance and 8% of the amplitude's, in both halves of
the night. The cycle must also
last at least three readings (2 s windows for the amplitude and the beat
error) and fit five times in the take (three for
the period search). A cycle at the alias of a period-search line too
fast for the readings is set aside: readings averaged over 10 s show a
3.75 s line (a 15-tooth escape wheel at 28,800 bph) at 30 s, and fold
almost as well at 60 s, 120 s or 330 s (and partly at half turns, 75 s).
A whole number of the alias's turns, or a half number that is also a
whole number of the line's own turns, matched within the resolution
(the period squared over the take, at least 1%, and at least 5% of the
alias), still counts only when folding there explains more than 1.25
times what the alias does, as the same watch's 60 s fourth wheel does.
On a long take that tolerance covers most periods above a few turns of
the alias (for the 30 s alias, every period above about 160 s on a 1 h
take, and above about 430 s on a 7 h take), so a real slow cycle there
must clearly outfold the alias to count unless the period search lists
it. On takes shorter than
twenty turns of the alias the 5% floor no longer sets the match at one
turn either, so it widens there too. A cycle the period search also
finds in the beat timing as a line of three readings or longer (within
the period squared over the take, at least 1%: the search's own rule for
matching a wheel) always counts, and is not reduced to a shorter
fraction that lands on the alias or its turns: at 19,800 bph a 15-tooth escape wheel turns in 5.45 s,
which aliases to exactly the fourth wheel's 60 s. At a turn and a half
or more of an alias the fold holds the alias as well, so the cycle's share and
size are given net of the alias's own fold (the readings less their
mean shape at the alias), and it must still explain 10%: on the 28,800
bph watch with a 15-tooth escape wheel, a 60 s fourth wheel of 2 s/d
peak to peak beside a 30 s alias of 2 s/d is given its own 2 s/d, not
the fold's larger swing, and a 60 s line the search lists with nothing
of its own in the readings is no cycle (the headline then
names it from the search). On the alias itself the two cannot be parted,
and the size is the fold's.

Folded at a period of an odd number of readings, n, the readings fall
in the same groups as at half of it: reading k is at phase k/n of the
one and 2k/n of the other, and with n odd both visit the same n phases,
in another order. So a 45 s cycle in 10 s readings can be found at
90 s, nine readings a turn. Which of the two the readings follow shows
in the shape's harmonics: a shape that repeats at half the period has
its variance in the even ones. When the period is known to under half
a reading (so that one whole number of readings is within the
resolution) and is seven readings or more, the cycle moves to half its
period if sines at the half and the quarter explain 85% or more of what
all n phases explain (sines at every harmonic up to n/2). At seven
readings a turn the quarter is under two readings and cannot be told
from the third in them, so the half is fitted alone. A line the
period search lists at an odd harmonic, such as a 30 s line beside
90 s, or a listed fast line's alias there, such as the 30 s alias of a
3.75 s escape wheel, is a line of its own sharing those phases and is
left out of the n phases first; a cycle the search lists at its own
period stays. The moved cycle's share and size are those of the sines
(a fold at 4.5 readings a turn puts readings of different phases in one
bin and reads low), and it must still explain 10% and clear a fast
line's alias, net of it at a turn and a half or more, as any cycle must.
Its source is where the period it was moved from was found.

The headline leads
with two levels, then a cycle, then level changes or a trend, then
correlation between readings. When the take shows more than the lead,
the headline adds a sentence. If one side's unlock edges or 2 s windows
fall in two clusters the other side's do not (the two-state finder's
direct check), the amplitude and beat-error headlines add that fact,
e.g. "Tick windows fall in two clusters; Tock windows do not", and
`unlock_hopping` is true.

Readings far from the slow trend are left out as outliers (a knock, a
lost beat). The limit is 6 σ from the 5-minute level, using the larger
of the reading-to-reading scatter and the spread about that level; the
text output states it beside the count. If more than 3% of the readings
would be left out, none are, because then they are part of what the
watch does.

## Isochronism and the 0 h / 24 h comparison

As the mainspring lets down, the amplitude falls and the rate moves with
it. The isochronism curve bins the rate's 10 s readings by the amplitude
of the same reading (2° bins, each with at least 10 readings, between the
2nd and 98th percentile of the amplitudes) and gives each bin's median
rate, its quartiles and when in the take its readings fall. The s/d per
degree is a straight line through every reading, none smoothed, so a
short swing of amplitude counts. The amplitude readings are noisy, and
noise in them shrinks a fitted slope towards 0, so the slope is divided
by their reliability: the share of their spread that is the amplitude,
1 less each reading's noise over their variance. The noise is read once
over the take, from the scatter between neighbouring 2 s windows within
a reading (1.4826·MAD/√2, so an event's edge inside a reading barely
moves it), times the sum of the squared weights (about 0.232 for five
windows). It comes
with a ± that allows for correlated readings, and is withheld below a
reliability of 0.5. It is a going-together, not a cause: anything else
that changes over the take moves with it, and a take in several positions carries a
`positions` caveat. No curve is fitted through the bins: a quadratic
has no source as an isochronism method, and the amplitude where it
turned came from the fit's form, not the watch (dropped 13 Nov). On a
short take the amplitude covers only a few degrees and the bins are
mostly noise; a run-down of many hours is what they are for. The cyclic co-movement inside
half an hour is the rate-to-amplitude line above, not this curve.

The isochronism walk (#741) keeps the time the curve leaves out: one
point per 5 min, the median rate of the 10 s readings in it and the
median amplitude of its 2 s windows (with their masks and culls), in
time order, so a run-down shows how the watch walks through rate and
amplitude as it lets down. Each series is read on its own, so a reading
missing one window still counts for the rate. A 5 min stretch with
under half its readings or under half its windows is left out, never
filled. Each surface merges the points to its own width
(`walk::merged`, the mean of each group's medians, each series weighted
by its own count), by default to at most 250 points a take. Bins of an hour or more
smear steps and pull the extremes in. It has no fit and no ±, and a
take in several positions mixes them along the way. A short take's
wheel cycle, which moves the rate and the amplitude together every few
minutes, shows in the rate-to-amplitude line, not here.

The wind-down comparison is the commercial 0 h / 24 h test for one
position. It compares the median rate, amplitude and beat error of the
first half hour of the take with the half hour that starts 24 h after
full wind, or with the take's last half hour when it ends sooner (it
needs at least two hours). `--wind-h H` says how many hours after full
wind the take started (default 0: it started at full wind). For several
positions, record two sessions, at full wind and about 24 h later, with
`wind_h` set in each (see [sessions.md](sessions.md)).

## Distributions

How often each value came up, as the app's Distributions pane draws it:
a histogram with the summary line the app shows beside it, and the values
at the shares marked on probability paper. They are counted over the
whole take and, for a take at least two stretches long, over each stretch
of `--stretch-min` minutes from its start (default 60; 0 for the whole
take only; a last stretch shorter than half joins the one before). Each
stretch and the whole take have five: the rate, amplitude and beat error
on the rate's readings (one per 10 s, as the tests use them), and
the amplitude and beat error on each 2 seconds of beats (quicker changes,
more scatter). The bins follow `timegrapher_core::histogram` with the
app's settings: Freedman–Diaconis widths rounded to 1, 2 or 5 times a
power of ten, never finer than 0.1 s/d, 0.5° or 0.01 ms, and the outer
0.5% each side left off the scale. The text output prints the whole take's summary lines.

## JSON (`timegrapher.series/3`)

The usual envelope (see [agent-interface.md](agent-interface.md)), then
the fields below. Version 2 removed the one-word `verdict` of each
series, the findings' `severity` and `advice`, and `rate_amplitude`'s
`moves_with`, and the `rate_moves_with_amplitude` finding with it; the
headlines state facts. Everything else is as in version 1.

| Field | Contents |
|---|---|
| `duration_s`, `bph` | the take |
| `caveats[]` | conditions detected, each with a `code`: `sides_by_count` (`count`: the breaks across which Tick and Tock were named by the beat count, not told apart by their sound) and `beat_error_as_size` (the beat error's sign differs either side of a break, so its readings are sizes, positive, not signed), as for `analyze`; empty when none |
| `config` | the detection settings used |
| `series[]` | one entry each for `rate` (s/d), `amplitude` (deg) and `beat_error` (ms, from the unlock), in that order |
| `isochronism` | the rate by amplitude: `step_s`, `bin_deg`, `bins[]` (`amplitude_deg`, `readings`, `rate_s_per_day`, `rate_q25`, `rate_q75`, `from_wind_h`, `to_wind_h`), `amplitude_from_deg`, `amplitude_to_deg`, `s_per_day_per_deg` and `s_per_day_per_deg_se` (null when withheld), `rate_change_s_per_day` (the slope over the bins' amplitudes; null when withheld), `reliability`, `headline` (`curve_centre_deg`, `curve`, `curve_explained` and `turn_deg`, the quadratic, were removed 13 Nov); null with fewer than three bins |
| `walk` | the rate against the amplitude by time: `bin_s` (300), `points[]` (`t_s`, the stretch's middle from the take's start; `wind_h`; `rate_s_per_day`; `amplitude_deg`; `readings`, the rate's 10 s readings; `amp_windows`, the amplitude's 2 s windows; `rate_slots` and `amp_slots`, how many of each the stretch could hold; `thin`, true when either was read from under 60% of its slots), in time order. A stretch whose rate or amplitude is read from under 10% of its slots is left out, as every window figure is withheld below that share. `rate_only_to_s`, when the rate is still read after the last point but the amplitude is not (a run-down's end), is the end of the last such stretch, seconds from the take's start; absent otherwise; null with fewer than two points |
| `wind_down` | `start` and `end` (`from_wind_h`, `to_wind_h`, `readings`, `rate_s_per_day`, `amplitude_deg`, `beat_error_ms`), `hours_apart`, `at_24h`, `rate_change_s_per_day`, `amplitude_change_deg`, `beat_error_change_ms`, `headline`; null under two hours |
| `distributions` | `overall[]`, `stretch_s` (0 when the take is not split) and `stretches[]` (`start_s`, `end_s`, `distributions[]`); each distribution has `series` (`rate`, `amplitude`, `beat_error`), `unit`, `values` (`readings` or `2 s`), `step_s`, `n`, `median`, `p10`, `p90`, `bin_width`, `edges` (one more than `counts`; bin i is from `edges[i]` up to `edges[i+1]`), `counts`, `below` and `above` (off the scale, counted in `n`), `peaks` (bin centres of separate peaks), `normal_line` (`centre`, `spread`: the straight line on probability paper; null with no spread), `quantiles` (`[share, value]` at 0.1%, 1, 5, 10, 25, 50, 75, 90, 95, 99 and 99.9% where the count resolves them), `points` (with `--views`, on the whole take's distributions only: the probability plot the app draws, `[value, z]` lowest first: each value against the normal score of its rank, every value up to 400 and evenly by rank beyond, the lowest and highest always kept), `summary` (the line beside the histogram, for files and pipes: an ASCII minus) and `screen_summary` (the same line as a screen shows it, each figure with a true minus, U+2212, its words as they are; the app and the MCP view show this one). A series with fewer than three values has no entry. |
| `rate_amplitude` | the rate against the amplitude: `step_s`, `readings` (pairs), `r`, `effective_readings`, `p_value`, `s_per_day_per_deg` and `s_per_day_per_deg_se` (within half an hour, corrected for the amplitude's noise as the isochronism's is; null below a `reliability` of 0.5), `reliability`, `take_r` and `take_s_per_day_per_deg` (whole take, no lag, drift included), `headline` (all of these as numbers, whatever their size) |
| `spectra[]` | the plain amplitude spectrum of each series, as a commercial timegrapher's FFT shows it: `series` (`rate`, `amplitude`, `beat_error`), `unit` (`ms` of timing for the rate, `deg`, `ms`), `step_s`, `period_s[]` (longest first) and `amplitude[]` (the size of the sine at each period, half its peak to peak), `method` (`hann_fft` when no reading is missing, `lomb_scargle` when some are; one method, see below), `support` (`finite_bins`, `total_bins`, and `description`, the sentence every surface shows for them), and `welch` when at least two segments fit: `segment_s`, `segments`, `period_s[]` and `amplitude[]`, the sizes averaged over Hann-windowed segments that overlap by half and each lie wholly inside an unbroken stretch, so nothing is filled in across a gap. The rate's also has `reading_swing`: `reading_s` (the readings' length), `period_s[]` (from twice a reading up) and `swing_s_per_day[]`, the swing peak to peak each line of the beat timing makes in those readings (a timing sine of *a* ms at period *P* swings the rate by 2·*a*·2π/*P*·86.4 s/d, of which a reading, the slope of a least-squares line through its beats, keeps 3(sin *x* − *x* cos *x*)/*x*³, *x* = π·reading/*P*), as the app's FFT and the MCP server's draw it. A segment is the largest power of two of readings that is at most a quarter of the series and fits in its longest stretch, and its longest period is half a segment. The rate's comes from the beat timing, so it reaches down to the escape wheel; the amplitude's and the beat error's from their 2 s windows. Each series has its slow change taken off as the period search takes it off; each size is the sine fitted to the readings present with Hann weights (Lomb–Scargle with weights, nothing filled in), which with every reading present is the Hann-windowed FFT's size; the points are thinned to at most 600 on a log period axis, keeping the largest in each slice so narrow lines survive. The period search reads the same Fourier transform but whitens it and sums harmonics to decide what stands out; this keeps the raw sizes, best drawn on log axes. |
| `time_frequency[]` | with `--maps`: the STFT maps of `rate`, `amplitude` and `beat_error`, then their SST maps, each `series`, `method` (`stft` or `sst`), `unit` (as `spectra`), `step_s` (the series' spacing: the beat timing's for the rate, 0.25 s on takes up to 12 h, 0.5 s up to 80 h, 4 s beyond, and 2 s windows for the others; for the SST, after a stretch longer than 65,536 readings is averaged down to that), `window_s` and `hop_s` (the STFT's 1200 and 300; null for the SST), `period_s[]` (48 rows, longest first, evenly spaced in log period), `t_s[]` (column times on the tests' time: each STFT window's middle, or the middle of each SST slice, at most 400), `values[][]` (one column per `t_s`, a size per `period_s`; null where nothing was read), and `scale`, the colour scale the app draws the map in (`linear` and `log`, each `lo` and `hi`: the sizes the darkest and the brightest colour stand for, the brightest also for every size above it; the top is the size 99.5% of the cells read stay under, the bottom 0 on the linear scale and at most 300 times smaller on the log; null when nothing was read). An SST column is the mean over its slice of the values read. A series too short for a map has none. |
| `coherence[]` | with `--maps`: the rate against the amplitude, then Tick's amplitude against Tock's, each `x` and `y` (the series: `rate` and `amplitude`, or `tick_amplitude` and `tock_amplitude`), `step_s` (10 s readings or 2 s windows, averaged down as for the SST), `period_s[]` (48 rows, longest first, as for the SST), `t_s[]` (the middle of each slice of time, at most 400), `coherence[][]` (one column per `t_s`, the mean coherence over the slice per `period_s`, 0 to 1; null where nothing was read), `phase_deg[][]` (the phase of `x` against `y`, −180 to 180: positive when `x` leads), `noise_95[]` (per period, the red-noise level above) and `noise_pairs`. A pair with no stretch long enough has no entry. |
| `lomb_scargle[]` | with `--maps`: one per series (`rate` in s/d, `amplitude`, `beat_error`), each `series`, `unit`, `readings`, `span_s`, `period_s[]` (longest first, at most 600 on a log axis, keeping the strongest point in each slice), `power[]` (0 to 1), `window[]` (the spectral window, 0 to 1) and `peaks[]` (strongest first, at most 8: `period_s`, `power`, `false_alarm`, `window_power` (the window's highest within one resolution width), `on_window_line`, `wheel`). A series with fewer than 20 readings has none. |
| `kurtograms[]` | with `--maps`: one per series (`rate` from the beat timing, `amplitude`, `beat_error`, read from the same series as `time_frequency`), each `series`, `step_s`, `window_s[]` (16, 32, 64 ... readings, at most 16,384, shortest first: one row each), `windows[]` (how many windows each row was read from, each with every reading present, stepped half a window; a row needs 8), `noise_sd[]` (how far a cell of the row strays under steady noise, 2/√`windows`), `period_s[]` (48, longest first, evenly spaced in log period, from 4 readings to a third of the longest window) and `values[][]`: the spectral kurtosis of each period at each window length, `mean |X|⁴ / (mean |X|²)² − 2` over the windows for the most powerful frequency in the period's band, null where the window holds under three turns. About 0 is noise, −1 a line of steady size, above 0 a line that comes in bursts: on windows no longer than a burst, the higher the less of the time it sounds, falling towards −1 on windows long enough to hold several bursts. A series with no row has none. |
| `wheel_lines[]` | per named wheel and series (`rate`, the beat timing in ms; `amplitude`; `beat_error`): `wheel`, `period_s`, `series`, `unit`, `window_s`, `bins[]` (lowest amplitude first: `from_deg`, the bin running 10° up; `windows`; `size` and `se`, in `unit`, the windows' sizes combined weighted by 1/`se`², the bin's `se` allowing for windows that overlap (two sharing a fraction *f* of their length correlate at *f*); a window's `se` is from its residuals about the fitted line and slow change, counted as fewer independent readings when neighbours follow one another, by the residuals' lag-one correlation and how strong such noise is at the wheel's own period (`confidence::effective_readings_at`); for the rate also `size_s_per_day` and `se_s_per_day`, 86.4 × 2π / `period_s` times the timing figures) and, with `--maps`, `windows[]` (`start_s`, `end_s`, `amplitude_deg`, `size`, `se`, `phase_deg`). A wheel with fewer than two windows in a series has no entry there. |
| `wheel_turns[]` | with `--maps`: per named wheel and series (`rate`, the beat timing in ms; `amplitude`): `wheel`, `period_s`, `series`, `unit`, `bins`, `all` and `by_amplitude[]` (lowest first), each `from_deg` and `to_deg` (null for `all`), `turns`, `profile[]` (from the turn's start), `se[]`, `peak_to_peak`. A wheel with fewer than ten turns read has no entry. |
| `period_span` | with `--views`: `[lo, hi]`, log10 seconds, the periods every series' search covers between them, so the Periods plots share one axis |
| `findings[]` | one per series where a test detected structure: `code`, `series`, `title` (what was detected), `evidence` (the headline and the statistics). Codes: `periodic`, `two_states`, `measurement_split` (two levels that show on one side only, or in the beat error from the unlock but not from the drop), `shifting_mean` (level changes), `drifting` (a trend), `wandering` (correlation between readings). A series where none was detected has none. |

Each entry of `series[]`:

| Field | Contents |
|---|---|
| `series`, `unit`, `headline` | what it is, and what the tests found in one plain sentence |
| `summary` | the headline's lead in a few words, one line, with no series name and no full stop, for a plot's title after the series' own: e.g. "repeats every 46 s, 4.00 s/d peak to peak", "shifts level 1 time: +4.9 s/d, then +9.1 s/d" (in the headline's own words, every level when there are four or fewer), "slides from 280° to 260°", "switches between 293° and 312°, about every 78 s", "successive readings correlated; no cycle, level change or trend", "no cycle, level change or trend", "20 readings, too few to search", and, for a cycle without one period to size, "drops to a low level and back, about every 78 s" or "repeats on a cycle". It says "no cycle" only when the headline names none. |
| `screen_headline`, `screen_summary` | the headline and the summary as a screen shows them: each figure's minus a true minus (U+2212), the words' hyphens as they are; the app and the MCP view show these, and files, pipes and the model read `headline` and `summary` (ASCII minus) |
| `screen_stats` | the statistics behind the headline on one line, as the app's Steadiness Info shows them: readings tested, outliers left out, median, SD, ± from one reading to the next, lag-1 r and Ljung–Box p, the trend per hour and the stretches of one level (each with why the headline leaves it out), and the period; each at the place the headline uses, with a true minus. The app and the MCP view show this one text |
| `step_s`, `readings`, `outliers` | reading length (the rate's for all three series), readings used, readings left out |
| `median`, `sd`, `short_term_sd`, `long_run_sd` | level, spread, reading-to-reading scatter, and the scatter allowing for correlated readings (the short-term σ × √(1+ρ)/(1−ρ), ρ the lag-one correlation once the largest step is out, bias-corrected), which the CUSUM is scaled by |
| `autocorrelation` | `lag_s[]`, `r[]` (thinned to about 600 points), `band`, `band_by_lag[]` (the band at each of `lag_s`, 1.96/√pairs from the pairs that lag was read from: wider than `band` where gaps leave fewer pairs; null where none), `ljung_box_q`, `ljung_box_lags`, `p_value`, `max_short_lag_r`, `memory_s`, `repeat_lag_s`, `repeat_r` |
| `allan` | `tau_s[]`, `deviation[]`, `white[]`, `pairs[]`, `slope_short`, `best_tau_s`, `best_deviation`, `max_excess` (largest deviation over the white line) |
| `cusum` | `t_s[]`, `s[]` (thinned, scaled by `long_run_sd`), `max`, `at_s`, `at` (the CUSUM there, signed), `p_value`, `band` (1.36: readings with no change of level, correlated or not, stay inside ±band about 95% of the time) and `shift` (the headline names a level change, so the level changes near `at_s`, the change finder's cut nearest `max`; `max` past the band alone does not set it). Without a cut, `at_s` and `at` are where `max` is |
| `changes` | `block_s`, `block_sigma`, `long_run_sigma` (the block medians' long-run σ, as `long_run_sd`, which each split is judged against), `segments[]` (`start_s`, `end_s`, `mean`), `explained`, `p_value` (the CUSUM's p that the level never moves, before Holm's correction, #917), `named` (whether the headline names the levels: a level change, or the steps of a slide) and `note` (when it does not, why: as for the trend's, "not significant: p …, over the 0.01 a level change needs", "not significant after Holm's correction (p … alone)", under the share a level change needs, or not the headline's finding; null when named) |
| `trend` | `per_hour`, `explained`, `curve_explained`, `start`, `end`, `turn` (`[time, value]` or null), `p_value` (the quadratic's F test on the readings' effective count, before Holm's correction), `significant` (whether that test passes `trend_alpha` and Holm's correction), `named` (whether the headline names the trend) and `note` (when it does not, why: "not significant: p …, over the 0.01 a trend needs" when its own test fails, "not significant after Holm's correction (p … alone)" when it passes that but not Holm's correction, under the share a trend needs, or not the headline's finding; null when named). The text output adds the note to the detail row, so a trend or levels row under a headline that names neither says so |
| `period` | the period search's strongest component (`other_periods_s`: the weaker ones, each also tried as a cycle): `period_s`, `significance` (−log10 false alarm), `beyond_simulated` (past p 0.0003, the smallest the null was checked to: the text says beyond the simulated range rather than quoting the tail's extrapolation, #1243), `explained`, `peak_to_peak` (for the rate, the swing of the rate itself from the slope of the average cycle, which 10 s readings show less of; see [long-runs.md](long-runs.md)), `timing_peak_to_peak_ms` (rate only), `size` and `size_unit` (the figure to show: ms of timing for a rate cycle under 30 s, see [long-runs.md](long-runs.md)), `wheel`, `harmonic_of` and `other_harmonic_of[]` (one per `other_periods_s`): null, or `period_s` and `n` when a longer cycle the search also found is n times this period (n 2 to 8, when n times the longer cycle's frequency is within one resolution, 1/span, of this one's, and only when one n and one longer cycle fit: neighbouring n are 1/q apart, so two can fit only when the longer cycle is over half the span, and then none is told) |
| `null` | the noise the period search is held to (#1195): `model` (`Readings`: AR(1) by the lag-one ρ the search whitens by; `Timing`, the rate: a random walk plus white jitter), `mix` (ρ for readings; for timing the crossover where the walk meets the jitter floor, f_c times the detrend's window, or the span under the quadratic; 0 for white, 1e12 when no floor shows), `calibrated` (whether the simulated table covers this take length, step and mix; when not, the search falls back to counting trials and the series carries `period_search_uncalibrated`); for timing also `misfit` (the rms, in decades, of the take's quarter-octave band powers from the walk plus white fitted to them, read with the named cycles taken out and their bands left out) and `misfit_limit` (the simulated null's 99th percentile of it for this take; absent outside the table) |
| `caveats[]` | the series' own conditions, each with a `code`, as the take's `caveats[]`; also `period_may_be_noise` (`rho`, `span_s`): on the amplitude or the beat error, when `period` names a cycle on a stretch under 2 h whose 2 s windows' lag-one ρ, the one the search whitens by (not the 10 s readings' lag-1 r), is 0.7 or more. There a slow wander under white measurement noise named a cycle in noise alone in up to 1 take in 10 (#916), and in 1 in 27 once the search is held to its simulated null (#1195). It changes no figure; the headline says it. The rate takes none. Also `period_search_uncalibrated` (`model`, `mix`, `span_s`): the period search named a cycle where the simulated null does not reach, so its false alarm is not known; the headline says so and no figure changes. Also `timing_model_misfit` (`misfit`, `limit`), on the rate: the period search named a cycle on beat timing whose spectrum sits further from a walk plus white than 99 in 100 simulated takes (`null.misfit` past `null.misfit_limit`; flicker, #1219), so its false alarm is not known there; the headline says so and no figure changes. |
| `two_state` | the two-state finder's full result (rate and amplitude only) |
| `cycle` | the cycle found, if any, from the 2 s windows for the amplitude and the beat error: `period_s`, `peak_to_peak`, `explained`, `source` (`two_state`, `autocorrelation`, `period_search`), `wheel`, and `inner_period_s`, `inner_explained`, `turns` when the period is a whole number of turns of a shorter cycle that explains at least half as much |
| `unlock_hopping` | amplitude and beat error only: one side's unlock edges or 2 s windows fall in two clusters the other side's do not |
| `views` | with `--views`: the chart data for each series besides the arrays above. `readings` (`step_s`, `n`, `chunk_s`, `t_s[]`, `values[]`, `lo[]`, `hi[]`, `counts[]`: the readings the tests were made on, on the tests' time (the clock's, with each pause left empty), 10 s for the rate and 2 s windows for the amplitude and the beat error, at most 3000 points; past 3000 readings, cut into 3000 near-equal chunks (chunk `i` starts at reading ⌊i·n/3000⌋, as a fold's raster is thinned), each point a chunk: the mean time and the mean of its readings with a value, their lowest and highest (the band the app draws) and how many there are, a chunk with none left out; `chunk_s` is a chunk's average length in seconds, `step_s` when every reading is its own point; `y_range`: the value scale the app draws them on, `[low, high]`, from every reading with a value, chunked or not, so a few stray readings stay outside it (a band reaching past it is clipped where it is drawn; `lo[]` and `hi[]` stay true), null with fewer than two; `banded`: whether any point stands for more than one reading, so each is drawn over its band; `key`: the view's key as the app writes it, "10 s readings (179)" or "10 s readings in 3000 bands (23400)", the count being the readings the bands hold); `search` (`period_s[]` longest first, `score[]`, `threshold`: the period search, at most 600 periods; null when there is none); `fold` (`period_s`, `from` (`own`, another series' name, or `wheel` with `wheel` naming it), `profile[]` (median per phase bin: one bin per phase the readings fall on when those repeat within two turns, at most 40; otherwise one per reading in a turn, 6 to 40), `cycles`, `raster[][]` (at most 200 of the cycles, evenly spread, one value per bin, null where empty), `columns[]` (each bin's quartiles over every cycle folded, `[lower, median, upper]`, null for a bin with fewer than four readings: the app's boxes), `swing` (the median line's highest less its lowest) and `middle_half` (the median of the columns' upper less lower), null unless both can be given; `y_range` (the value scale the app draws the fold on, `[low, high]`); null when no period fits) |

`--views` (with `--json`) adds the chart data: the `points` and `views`
above, a few hundred kB on a long take (about 600 kB over 2 h 40 min), so
plain `--json` stays small for agents. The MCP server asks for them and
keeps them out of what the model reads.

`--maps` (with `--json`) adds `time_frequency`, `coherence`,
`lomb_scargle`, `wheel_turns`, `kurtograms` and the `wheel_lines` windows, about 4.1 MB on a 2 h take (parts 6 and 7 of the Pepsi replica's
take, Dandong 3285, where `--views` adds about 0.7 MB).

The JSON's top-level `clock` describes the correction actually applied to
these readings: `ppm`, `rate_error_s_per_day`, `from` (the source), and
`tracks_drift`. It is `null` when no correction was applied. An explicit
`--clock` log takes precedence; otherwise the stored correction for the
input named by the first recording's provenance is used, then USB lines
from that recording if available. A stored zero correction is still an
applied correction. There is no `--card-ppm` or `--device` option for `series`.
When the recording has USB lines, they set the rate even with a
`--clock` log: they time the card against the computer's USB clock to
about 0.1 ppm, where a log of an entry a minute for half an hour
measures it to about 13 ppm. The log then only places its breaks; `from`
names both, and `withheld` gives what the log measured with `reason`
`usb_lines`. Without USB lines, a `--clock` log that can't measure the
clock well enough to correct by (under 10 entries, or a standard error
over 20 ppm, as a log of a minute or two gives) still places its breaks
but corrects nothing: `ppm` and `rate_error_s_per_day` are 0 and
`withheld` gives what it measured, `ppm`, `ppm_sd`, `points`, `span_s`
and `reason` (`few_entries` or `unsure`). The same rules hold in `long`,
`session` and the power reserve reader.
Positive ppm means the card runs fast and uncorrected rates read slow;
`rate_error_s_per_day` is −ppm × 0.0864. For a log that follows drift, these
numbers summarize the fit; its time mapping need not be one uniform
correction. These fields do not claim calibration uncertainty or accuracy.

Times are seconds from the start of the take, mapped through the selected
clock fit when `clock` is non-null. The arrays are meant to be drawn as they are: the
autocorrelation with its band, the Allan deviation against `white` on
log-log axes, and the CUSUM with lines at ±1.36.

## Synthetic takes

`timegrapher synth --scenario NAME FOLDER` writes a take with one known
variation hidden in it, to check what each view shows; see
[synthetic-takes.md](../developer/synthetic-takes.md).

## On the stored takes

Run on main plus this command, at the lift angles the takes' READMEs
give, with the clock log where the take has one. The watches are
replicas with clone movements: the Yacht-Master 42 replica (Dandong 3235
movement), the Daytona replica (Dandong 4130) and the Reverso Duo replica
(21,600 bph movement); the Dandong 3235 is a different watch.

| Take | Rate | Amplitude | Beat error |
|---|---|---|---|
| Yacht-Master crown left (ym42_CL_90min) | two states, +44 and +75 s/d, switching about every 35 s; also a 60 s cycle (fourth wheel), 26 s/d peak to peak, 41% of the variance | 60 s cycle (fourth wheel), 7° | 60 s cycle (fourth wheel), 0.25 ms |
| Yacht-Master crown right, part a | two states, −20 and −7 s/d, about every 35 s; also a 257 s cycle, 15 s/d | 4 min cycle, 6°; Tick windows fall in two clusters, Tock windows do not | slow change; it flips sign at the suspend gap (1,557 s), where Tick and Tock swap |
| Yacht-Master crown right, part b | two states, −17 and −4 s/d, about every 30 s; also a 60 s cycle (fourth wheel), 10 s/d | 60 s cycle (fourth wheel), 8° | 60 s cycle (fourth wheel), 0.06 ms |
| Yacht-Master crown up (ym42_CU_8h, 2 h 17 min) | two states, −15 and −8 s/d, about every 50 s; level settles from −18 to −9 s/d | 60 s cycle (fourth wheel), 5° | successive readings correlated |
| Yacht-Master dial up (ym42_DU_2h) | 60 s cycle (fourth wheel), 12 s/d, 18% of the variance; the level settles from +55 s/d at the start to +6 s/d | two states, 241° and 250°, about every 40 s; also a 60 s cycle (fourth wheel), 11° | 60 s cycle (fourth wheel), 0.09 ms |
| Dandong 3235 (dd3235_DU_30min) | slow change, −4 s/d in the first 10 min to +2 s/d | nothing detected (±16° from window to window would hide any small cycle) | nothing detected |
| Daytona 4130 (`--escape-teeth 20`) | 60 s cycle (fourth wheel), 2.8 s/d, 82% of the variance | 60 s cycle, 17°; Tick windows fall in two clusters, Tock windows do not | 60 s cycle, 0.35 ms; the same Tick clusters |
| Reverso, whole 2 h | 48 s cycle, 0.8 s/d; level moves between +18 and +21 s/d | 4 min cycle, 10°, beside a 48 s one; level rises from 237° to 256° and falls back to 242° | successive readings correlated |
| Reverso, file 01 | two states, +17 and +22 s/d, about every 50 s; also a 47 s cycle, 2.8 s/d | 48 s cycle, 12° | successive readings correlated |

The two-state results agree with `session`, and so does the Daytona's
once-a-minute rate cycle. The Yacht-Master's fourth-wheel cycles are
new. Crown left, the two rates follow the fourth wheel's turn, a cycle
the two-state finder's 10 s blocks cannot resolve as regular. Its
amplitude dips once a minute crown left, crown up and dial up as well.
That points at something that turns once a minute: the seconds hand, or
the fourth wheel and its pinion.

### FFT input availability

Each spectrum's `support` counts finite samples in the actual detrended FFT input
grid, before next-power-of-two padding. Nonfinite samples (NaN or infinity) count
as missing. These are grid-sample counts, not acoustic coverage or the percentage
of watch beats heard; complete finite input does not establish continuous sound.
The app and MCP FFT show these counts. Older chart documents without counts
state that support was not recorded.

Every size is read one way: at each period, the sine fitted by least squares to
the samples present, each weighted by a Hann window over the whole grid, less
their weighted mean (Lomb–Scargle with weights, its sums read off the transforms
of the weighted samples and of the weights present, as Press and Rybicki 1989).
Nothing is filled in. With every sample present this is the Hann-windowed FFT's
size (`method` `hann_fft`); with any missing (`method` `lomb_scargle`) a line
keeps its size as samples go missing, and the window keeps a strong line's
sidelobes as short as on a complete grid, so one sample out leaves the spectrum
as it was. A gap does lose what it held: gaps that recur put lines of their own
in the spectrum (the `lomb_scargle` section's `window[]` shows where), a block
gap of a few percent can move the largest bin of a strongly correlated series,
and at the shortest periods of a series whose readings follow each other
closely (lag-one correlation near 0.99), where it holds least, even one sample
out mid-take can lift a stretch of the spectrum by about 10%. Counts are
neither a correction factor nor an uncertainty estimate. Eligibility and the
period search are unchanged.

When MCP moves spectrum arrays into chart metadata, compact `spectrum_support`
entries (`series`, `unit`, `support`) remain in the model-visible structured result
and text lead. Legacy spectra have `support: null` there; no completeness is inferred.
