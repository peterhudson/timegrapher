# Synthetic takes

`timegrapher synth --scenario NAME FOLDER` writes a take with one known
variation hidden in it, as a take folder the app, `series` and `long`
open like a recording: two hours as twelve 10-minute FLAC files,
`session.json`, and a README saying what is hidden and what the analysis
should find. `--scenario list` names them, and `--seed` changes the
noise (the same command always writes the same sound). The watch is
synthetic: 28,800 bph laid out as an ETA 2824-2 (a 20-tooth escape
wheel turning in 5 s, the fourth wheel in 60 s), lift angle 50°, dial
up, rate +5 s/d, amplitude 270° and beat error 0.4 ms, with a
beat-to-beat spread of 0.25 ms in timing and 3° in amplitude, about what
a good watch on a quiet stand shows (each 10 s rate reading then
scatters by about 0.9 s/d). Choose the ETA 2824-2 as the calibre so the
views name its wheels.

| Scenario | Hidden | Headline (`series`, seed 1) | Where it shows |
|---|---|---|---|
| `control` | nothing | "Rate: 719 10 s readings, median +5.0 s/d, ±0.9 s/d from one reading to the next; no cycle, level change, trend or correlation between readings detected." (amplitude and beat error alike) | every view stays inside its noise bands |
| `fourth-wheel` | rate ±1 s/d every 60 s | "Rate repeats every 60 s (the fourth wheel), 1.8 s/d peak to peak." | Periods, FFT, Cycle and the autocorrelation's repeat at 60 s |
| `escape-wheel` | rate ±10 s/d every 5 s (0.18 ms of timing) | "Rate: … the beat timing repeats every 5.0 s, the escape wheel's period, 0.18 ms peak to peak; 10 s readings are too coarse to show it." | FFT with Timing; not in the readings, since a 10 s reading is two whole turns |
| `half-period` | rate ±1 s/d every 45 s | "Rate repeats every 45 s, 1.7 s/d peak to peak." | Periods, FFT and Cycle at 45 s, four and a half readings a turn |
| `slow-cycle` | amplitude ±3° every 8 min | "Amplitude repeats every 8 min, 6.6° peak to peak." | Periods, FFT and Cycle at 480 s, and a bump in the Allan deviation near 240 s |
| `beat-error-cycle` | beat error ±0.1 ms every 120 s | "Beat error repeats every 120 s, 0.22 ms peak to peak." | Periods, FFT and Cycle for the beat error |
| `letting-down` | amplitude 280° falling to 260° | "Amplitude slides from 279° to 259° (-10.0° per hour)." | Trend, and an Allan deviation that rises at long τ |
| `knock` | rate +5 to +8 s/d at 70 min | "Rate shifts level 1 time: +5.0 s/d, then +8.1 s/d." | CUSUM's corner and Change points at 70 min |
| `wander` | rate random walk, about 1 s/d per 10 min | "Rate shifts level 5 times, between -0.2 s/d and +4.5 s/d: +4.5 s/d at the start, +1.8 s/d at the end." | the change finder splits a walk into steps; a slow-falling autocorrelation; Allan deviation above the white line |
| `rub` | for 8 s every 180 s, rate −30 s/d and amplitude −15° | "Rate repeats every 3 min, 26.8 s/d peak to peak." and "Amplitude repeats every 3 min, 15.2° peak to peak." | Periods, FFT and Cycle at 180 s; Two states shows the low level |

The headlines are from the sound, through the whole analysis. A test
(`scenarios::tests`) runs each scenario's beats and 2 s windows, made
straight from its tracks with the same spread, through the steadiness
analysis and checks that every headline starts as the table's does and
that the series with nothing hidden find nothing.

## Reading the FFT: rate against timing

The FFT pane's rate is one spectrum drawn two ways. **Timing** shows the
beat timing's own swing in ms at each period. Without it, each timing
swing is drawn as the swing it makes in the 10 s rate readings, in s/d:
the rate is the timing's slope, so a timing sine of *a* ms at period *P*
swings the rate by 2·*a*·2π/*P*·86.4 s/d (less what a 10 s reading
averages away). The same timing swing is a smaller rate swing the
slower it is. On `fourth-wheel` the line is 0.10 ms of timing at 60 s,
1.8 s/d of rate; 0.10 ms at 30 min would be 0.06 s/d. So the two views'
tallest points need not agree: slow wander, which builds up in the
timing toward the longest periods the take can show, tops the Timing
view, while a wheel's line tops the rate view. Timing is the view for
fast lines such as the escape wheel's; the rate view for the rate's own
cycles.

## Reading a Cycle fold

The Cycle pane folds a series at a period: each reading is placed at
its point in the cycle, and the line joins the median at each point.
Readings stepped 10 s apart land on period ÷ 10 s points, so the dots
stack into columns, one dot per turn. A column's height is the
reading-to-reading scatter (and any slow change over the take); the
line is what repeats. With *n* dots in a column, its median wanders by
about the column's SD ÷ √*n* from noise alone, so a line that rises and
falls by many times that follows the cycle, and one that moves by only
a few times it does not.

On `fourth-wheel` (2 h, seed 1) the rate folded at its own 60 s cycle
has six columns of 120 dots, each about 2.7 s/d tall from its 5th to
its 95th percentile (SD 0.86 s/d, so a median wanders by about
0.08 s/d), and the line rises and falls by 1.9 s/d: the cycle. The
amplitude, which has no cycle, folded at the same 60 s (the pane says
"Folded at the rate's 60 s cycle") has thirty columns of 120 dots about
2.7° tall (SD 0.83°, a median wandering by about 0.08°), and its line
moves by 0.35°: what thirty medians of noise spread to, so no 60 s cycle
in the amplitude.
