# 17. Glossary and Sources

## Glossary

**Allan deviation**: a measure of how successive averages differ, plotted
against averaging duration. It helps distinguish short-term noise from
behaviour that stops averaging away. It is not the standard deviation of
all readings, nor a confidence interval on their mean. See
[Allan Deviation](09-steadiness.md#allan-deviation).

**Amplitude**: the angular excursion of the balance from equilibrium to
a turning point, in degrees. Timegrapher estimates it acoustically using
lift time, beat period and the supplied lift angle. See
[Amplitude](01-how-it-works.md#amplitude).

**Autocorrelation**: similarity between a series and a delayed copy of
itself, as a function of the delay or **lag**. Repeated positive and
negative lobes can reveal periodic structure.

**Average Over**: the interval used for the ordinary displayed readings.
It does not set the independent analysis windows used in Steadiness and
Isochronism. See [Average Over](05-readings.md#average-over).

**Banking**: the limits to the pallet lever's travel. Contact with the
banking contributes to the final acoustic group of a beat.

**Beat**: one vibration, or half oscillation, of the balance. A complete
back-and-forth oscillation contains two beats.

**Beat error**: the departure from equal spacing of alternating beats.
The reported convention is half the difference between the long and
short intervals. The main reading uses Unlock when available and the
drop-based reference as a labelled fallback. See
[Beat Error](01-how-it-works.md#beat-error).

**Beat rate / bph**: the number of beats per hour. A 4 Hz oscillator makes
eight beats per second, or 28,800 bph. This nominal frequency is distinct
from the measured gain or loss in s/d.

**Calibre**: a movement design or model designation. Catalogue information
must be checked against the particular version being measured.

**CH, CB, 3H, 6H, 9H, 12H**: Swiss position notation. CH is dial up; CB is
dial down. For vertical positions, the hour numeral named points upward.
The correspondence with crown directions assumes a particular crown
location. See [The Six Positions](12-positions.md#the-six-positions).

**Clipping**: truncation when the signal exceeds the recording system's
available range. It can alter the envelope and compromise amplitude,
beat error and, in severe cases, beat detection or rate.

**Confidence interval**: an interval constructed to have a stated
coverage under specified assumptions. The app's ± estimates rely on a
noise model; they do not include all systematic error or future changes
in the watch. See [What the ± Means](05-readings.md#what-the--means).

**CUSUM**: a cumulative sum of departures from a reference level. The
Steadiness pane uses departures from the whole-series mean, normalised by
short-term scatter and sample count. It highlights sustained changes;
its vertical axis is not accumulated time gained.

**Cycle folding**: assigning observations to their phase within a chosen
period and overlaying successive cycles. A visible folded plot is not,
by itself, evidence that the period was detected.

**D**: the range of the measured position rates: fastest minus slowest.

**dBFS**: decibels relative to full-scale digital signal amplitude. 0 dBFS
is full scale; −10 dBFS is about 0.316 of full-scale amplitude. The
normalised dB scale in the beat profile is a different reference.

**Drop**: mechanically, the escape wheel's free advance after leaving
one pallet's impulse face and before locking on the opposite pallet.
The app's **Drop** label names the final acoustic group associated with
locking and banking, and its detected marker.

**DU, DD, CU, CD, CL, CR**: dial up, dial down, crown up, crown down,
crown left and crown right. Record the physical orientation when crown
location makes the shorthand ambiguous.

**DH / DV**: the range of rates within the horizontal positions (DH) or
within the vertical positions (DV).

**Di**: crown-left rate minus dial-up rate, corresponding to 6H minus CH
with the conventional crown-at-3-o'clock mapping.

**DVH**: the mean of the vertical-position results minus the mean of the
horizontal-position results, calculated for rate and amplitude.

**Envelope**: a representation of the changing strength of an acoustic
signal. The Tick Tock Profile displays aligned, averaged envelopes,
not the balance's motion or a direct plot of escapement force.

**FFT**: fast Fourier transform, an algorithm for resolving a sampled
signal into frequency components. The FFT pane plots component size
against period. Its units and amplitude convention depend on the series;
see [FFT](09-steadiness.md#fft).

**Harmonic**: a component at an integer multiple of a fundamental
frequency. Twice the frequency means half the period. Several harmonics
can belong to one non-sinusoidal repeating disturbance.

**Ie**: the absolute change in mean rate between the full-wind and later
position tests, using positions present in both.

**Im**: the signed rate change with the largest absolute magnitude among
eligible positions in the two tests; the 12H position is excluded by the
convention used here. See [Comparing Full Wind and 24 Hours](12-positions.md#full-wind-against-24-hours).

**Impulse**: energy transferred from an escape-wheel tooth acting on a
pallet stone's impulse face, through the pallet lever and fork, to the
balance's impulse pin. **Impulse** in the app is also the label for the
corresponding acoustic group.

**Impulse pin / roller jewel**: the jewel carried by the balance roller
that engages with the pallet fork during unlocking and impulse.

**Interquartile range (IQR)**: the interval between the 25th and 75th
percentiles. In Isochronism it shows the middle half of the observations
within a bin, not uncertainty in that bin's median.

**Isochronism**: constancy of oscillation period as amplitude changes.
The app examines the observed relationship between rate and amplitude;
that relationship alone does not isolate amplitude from other changing
conditions. See [Isochronism](10-isochronism.md).

**Jitter**: scatter of individual beat times around a fitted timing model,
usually expressed in microseconds. It can contain both physical
variation and uncertainty in detecting the acoustic reference.

**Lift angle**: the balance's angular travel through the escapement's
lift action. It is a movement-specific geometric input to the amplitude
calculation, not an angle measured automatically from the microphone.

**Ljung–Box test**: a test for serial correlation over a group of lags.
A small p-value is evidence against the test's uncorrelated reference
model, not the probability of a mechanical fault.

**Locking face**: the surface of a pallet stone on which an escape-wheel
tooth is held between releases. Distinguish it from the impulse face.

**NTP**: Network Time Protocol, used to synchronise computer clocks. The
quality of that reference affects calibration against system time.

**Oscillation**: one complete back-and-forth motion of the balance,
containing two vibrations or beats.

**Paper Strip**: a trace of individual beat timing in the style of a
printing timegrapher. Its slope follows rate relative to the selected
reference. See [The Paper Strip](06-paper-strip.md).

**Period**: the time taken for one repetition. In a spectrum it is the
reciprocal of frequency: a 60-second period is 1/60 Hz.

**ppm**: parts per million. A reference-clock error of 10 ppm corresponds
to about 0.864 s/d of rate error.

**Rate**: the measured departure from nominal frequency, expressed as
seconds gained (+) or lost (−) per day if that behaviour continued.

**Reading-to-reading scatter**: a scale estimated from differences
between neighbouring readings. It can be much smaller than whole-series
SD when a level drifts slowly. It is not the uncertainty of the mean.

**Residual**: what remains after subtracting a fitted model. Timing
residuals show beat departures after the overall rate and alternating
side offset have been accounted for.

**Sample clock**: the clock determining the audio sampling rate. It
provides the immediate time reference for measurements from the audio.

**SD**: standard deviation, a measure of spread around the mean. A large
SD may arise from noise, drift, steps or cycles; it does not identify
which of those occurred.

**Session**: in the desktop app, a live acquisition or opened recording
with its associated state. In the command-line `session` command, a set
of position takes analysed together.

**Settling**: time excluded after changing position before taking a
measurement. An ordinary position change uses ten seconds; the guided
Positions Test defaults to twenty seconds and allows adjustment. A new
session created by CLI `record` or MCP `record_position` defaults to
thirty seconds per take; existing manifest settings can differ.

**Signal**: the reported ratio of a typical beat's strength to the
background estimate. It helps assess acquisition, but is not a direct
signal-to-noise measurement of every feature needed for amplitude.

**Tick / Tock**: the software's names for alternating beats, coloured
blue and orange. They do not establish which pallet is involved and may
exchange on reacquisition.

**Timing scatter**: fit residual scatter expressed relative to the beat
period. Too much scatter prevents a regular-beat result; it is separate
from amplitude and from the long-term stability of the watch.

**True Clock**: the rate after applying an estimate of sample-clock error.
The name describes a correction, not a guarantee of absolute accuracy.
See [True Clock](05-readings.md#true-clock).

**Unlock**: release of the escapement initiated by the balance impulse
pin moving the pallet fork. The first acoustic group begins with the
pin's contact with the fork; its detected marker is an acoustic estimate,
not a direct observation of the tooth leaving the locking face.

**Vibration**: one half oscillation of the balance; one beat.

**X / XH / XV**: the mean rate of all measured positions (X), horizontal
positions (XH, also shown as H), or vertical positions (XV, also shown as V).

## Sources and Further Reading

The mechanical definitions, measurement conventions and statistical
methods below have different kinds of evidence. The sources explain the
principles; the behaviour of this particular app was checked against its
implementation. A source describing a commercial instrument does not
establish that Timegrapher has the same performance.

- **Witschi, _Measuring Technology and Troubleshooting for Watches_,
  training course, document 71.1010D35e, release 1.1 (2010).**
  [PDF hosted by Welwyn Watches](https://www.welwynwatches.co.uk/uploads/9/1/4/3/9143536/witschi_training_course.pdf).
  It explains the three acoustic groups, rate, amplitude and beat error,
  the words early and late for the paper strip (p. 6) and the scope spans
  (p. 12).
- **Horopedia, _Distribution Mechanism — Swiss Lever Escapement_.**
  [Illustrated mechanical sequence](https://horopedia.org/distribution-mechanism-swiss-lever-escapement/).
  Particularly useful for separating the tooth's action on the pallet
  stone, drop, locking and the lever's run to the banking.
- **Witschi, _Testing Methods for Mechanical Watches_ (2025).**
  [Official PDF](https://www.witschi.com/wp-content/uploads/firmwares/8Z1UtLGf/20251118_witschi_testing-methods-for-mechanical-watches_en.pdf),
  available from Witschi's [training resources](https://www.witschi.com/en/service/uhrmacherinnen/).
  It describes Im and Ie, and gives practical measurement advice (the
  20-second stabilisation, about p. 34). The NIHS 93-10 convention discussed in this guide is based on
  Witschi's account; this guide does not reproduce or independently
  establish compliance with that standard.
- **_Handbook of Frequency Stability Analysis_, NIST Special Publication
  1065 (2008).**
  [NIST PDF](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication1065.pdf).
  Section 5.2.4 covers overlapping Allan variance. This is background for
  understanding the statistic, not a claim that the app's watch data are
  a laboratory frequency standard.
- **tg, the open-source timegrapher.**
  [Source repository](https://github.com/vacaboja/tg).
  The project uses it as an independent measurement comparison; see
  [Validation](../validation.md) for what was compared and the limits
  of that evidence.
- **Movement-specific references.** Each catalogue entry records its
  own sources. Read them through **Manage Calibres…** or
  `timegrapher calibres NAME`, particularly when verifying lift angle,
  tooth counts or a movement variant.

For the implementation's statistical choices and validation, read
[Measurement Precision](../measurement-precision.md),
[Time-Series Analysis](../series.md) and [Long Runs](../long-runs.md).
The [watchmaker skill](../../skills/watchmaker/SKILL.md) has further
references for interpreting observations in the context of a watch.

---

Previous: [Problems and Reports](16-problems-and-reports.md) ·
[Back to the contents](README.md)
