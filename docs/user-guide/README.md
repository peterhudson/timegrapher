# Timegrapher User Guide

How to measure a mechanical watch with Timegrapher: every button, pane
and function, what it does, and why you'd use it.

Timegrapher states measured facts. It doesn't grade a watch or say what
is wrong with it. Interpreting the facts is the watchmaker's job; this
guide explains what each fact means and where it can mislead.

## Contents

**Start**

1. [What a Timegrapher Measures](01-what-a-timegrapher-measures.md): the beat, its three sounds, rate, amplitude and beat error
2. [Your First Take](02-your-first-take.md): Quick Listen, from install to a saved take
3. [Setting Up the Microphone](03-setting-up-the-microphone.md): the input, the level, clipping, automatic gain and Quartz Correction

**The App**

4. [The Window](04-the-window.md): the top row, Bench and Watches, panes, the status bar, menus and keys
5. [The Readings Cards and the Calibre Card](05-the-readings-cards.md): the position, the calibre, Listen, the readings, their ± and chips
6. [The Paper Strip](06-paper-strip.md): reading the strip, moving around, the track, masking and the Position Brush
7. [The Tick Tock Profile and the Scope](07-profile-and-scope.md): the average beat and its marks, and single beats
8. [Charts and Distributions](08-charts-and-distributions.md): each reading over the take, and how its values spread

**Steadiness**

9. [Steadiness: What It Is For, the Toolbar and Info](09-steadiness.md): the nine views, the Info panel, what the tests read and how sure they are
10. [Readings Over Time](10-readings-over-time.md): every reading in order, its levels and two states
11. [Periods](11-periods.md): what the readings repeat at, matched to the wheels
12. [Cycle](12-cycle.md): one repeat's shape and size
13. [Autocorrelation](13-autocorrelation.md): whether each reading follows the last
14. [Allan Deviation](14-allan-deviation.md): how the scatter of an average changes as it gets longer
15. [CUSUM and Change Points](15-cusum-and-change-points.md): whether the level changed, and when
16. [FFT](16-fft.md): the size of every period's swing over the whole take
17. [STFT Map](17-stft-map.md): each period's size through the take, in 20-minute windows
18. [Wavelet Map](18-wavelet-map.md): where and when each swing is, sharply

**Tests and History**

19. [Isochronism](19-isochronism.md): the rate at each amplitude, as the curve or the walk
20. [Positions Test](20-positions-test.md): the six positions, guided, and full wind against 24 hours
    - 20b. [Dynamic Poise Test](20b-dynamic-poise-test.md): the eight upright positions, 45° apart, the once-round wave and the heavy spot
    - 20c. [Complication Test](20c-complication-test.md): a chronograph or other complication off, on, then off again, and what it changes
21. [Power Reserve Test](21-power-reserve-test.md): how long a full wind lasts, hour by hour
22. [The Watches View](22-watches-view.md): your watches, their details and every take
23. [Takes and Sound Files](23-takes-and-sound-files.md): keeping, opening and what is in a take
24. [Settings and Calibres](24-settings-and-calibres.md): General, the Watches folder, and Manage Calibres

**How the Numbers Are Made**

Each page says what a measure is, how it is computed, why it is done
that way, when you'd use it, what it helps find, and what it can't tell
you.

- [M1. Beats](m1-beats.md)
- [M2. Rate](m2-rate.md)
- [M3. The Sound Card's Clock](m3-the-sound-cards-clock.md)
- [M4. Amplitude and Lift Angle](m4-amplitude-and-lift-angle.md)
- [M5. Beat Error](m5-beat-error.md)
- [M6. Averaging, Windows and Cuts](m6-averaging-windows-and-cuts.md)
- [M7. The ± and Correlated Readings](m7-the-plus-minus.md)
- [M8. Positions Test Figures](m8-positions-test-figures.md)
- [M9. Steadiness Statistics](m9-steadiness-statistics.md)
- [M10. Finding Cycles](m10-finding-cycles.md)
- [M11. Time and Frequency Maps](m11-time-and-frequency-maps.md)
- [M12. Isochronism and Rate against Amplitude](m12-isochronism-and-rate-against-amplitude.md)
- [M13. Dynamic Poise](m13-dynamic-poise.md)

**Beyond the App**

25. [The Command Line](25-command-line.md): every command, its options, and its JSON (JavaScript Object Notation) output for scripts and agents
26. [Claude Desktop](26-claude-desktop.md): the timegrapher in an AI (artificial intelligence) chat app, what it can do and what it never does
27. [Problems and Reports](27-problems-and-reports.md): Report a Problem, the log and crash records, and a window that won't open
28. [Glossary](28-glossary.md): the words this guide uses

## Other Documents

- [Installing and updating](../reference/install.md)
- [The desktop app reference](../reference/app.md): every detail of the app, for
  checking a specific behaviour.
- [The agent interface](../reference/agent-interface.md): the command line's JSON.
