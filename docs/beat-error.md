# Beat error: magnitude and signed timing

Every ordinary BE plot and reading shows a magnitude in milliseconds. A change
of Tick/Tock labels must not look like a change in beat error, and the display
must not depend on recognizing every missed beat or distinguishing similar beat
shapes across a gap.

## Where the magnitude is taken

The timing fit and unlock correction stay signed. The unlock calculation first
combines the signed drop error with the difference between the two unlock
positions. Taking absolute values before that calculation would be wrong.

Once each local estimate has been fitted (normally the existing 2-second
amplitude window), its magnitude can be used independently of the side labels.
Displayed readings and whole-take summaries take the median of these magnitudes,
separately for unlock and drop. They do not take the magnitude of a signed
median: windows of −0.5 and +0.5 ms must summarize to 0.5 ms, not zero. The BE
standard error is computed from the same magnitude windows.

A live reading with no complete amplitude window and at most 10 seconds of
sound can use its local drop fit as a fallback. A beat-only session can use the magnitudes of its existing
local rate-window fits. Neither fallback takes the magnitude of a whole-take
signed average.

## Plots and subsequent analysis

The strip overlay, Charts, histograms, probability plots, and report charts use
magnitudes. The shared BE series takes magnitudes **before** rebinning into
longer readings, calculating trends and changes, distributions, wind-down,
correlations, cycles, spectra, time-frequency maps, or wheel lines. Centred
residuals, slopes and changes may still be negative; those are changes in the
size of BE, not negative BE readings.

These analyses now describe changes in BE magnitude. A real signed oscillation
through zero folds when expressed as a magnitude and can produce harmonics or
change the apparent period. Near zero, taking magnitudes also folds measurement
noise upward. This is not a new noise correction or a claim of extra precision.
The magnitude is taken after the existing local fit, never on individual noisy
raw tick intervals.

Masks and breaks are applied before combining windows. Missing readings stay
missing. This change does not add a spike filter, change beat detection, recover
side identity, or modify recordings; unwanted disturbances can be masked as
before.

## Signed data retained for diagnostics

`TimingFit`, `AmplitudeWindow`, unlock/drop edge positions, beat indices, and the
saved analysis-only window CSV retain their signs. The Tick Tock Profile uses
the signed drop fit to place the two waveforms on one clock. Side-specific
unlock-hopping diagnostics also retain their signed edge relationships.

High-level `analyze`, `session`, `long` and live readings add
`beat_error_magnitudes: { unlock_ms, drop_ms }`. Both values are nonnegative or
null; prefer unlock when present. This is the authoritative result for display
and comparison with a bench timegrapher. The existing `beat_error_unlock_ms`,
`beat_error_ms` and `overall.beat_error_ms` fields retain their previous diagnostic
semantics for compatibility, including the existing combination across known
breaks. Their `beat_error_as_size` caveat describes those legacy fields; it does
not control whether a plot uses magnitudes. Old stored documents without the new
object can still be displayed using the magnitude of their saved scalar.

Raw per-window CSV exports stay signed. The session readings CSV uses the
magnitude summaries in its existing BE columns. `long` slices expose magnitude
columns alongside the existing diagnostic columns. The BE `series` output, including its
views and statistical results, always describes magnitudes.
