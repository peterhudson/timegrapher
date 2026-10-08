# Validation

## Synthetic recordings

`cargo test` generates recordings with known values and checks that they
come back out (`crates/timegrapher-core/tests/synthetic.rs`):

| Check | Tolerance |
| --- | --- |
| Beat rate guessed at 18,000, 21,600, 28,800 and 36,000 bph | exact |
| Rate (−7 s/d set) | ±0.3 s/d |
| Beat error (0.6 ms set) | ±0.05 ms |
| Beat error from the unlock (drops 0.4 ms apart, one side's unlock 0.6 ms earlier: −0.2 ms) | ±0.05 ms, sign included |
| Amplitude (220–280° set) | ±8° |
| Rate loses 40 s/d for 10 s of every minute | 60 s period found, named "fourth wheel" |
| Amplitude dips 30° for 8 s of every minute | 60 s period found |

The synthetic signal is much cleaner than a real microphone, so these
tests show the arithmetic is right, not that real recordings will be easy.

## Against tg on a real recording

Recording: Rolex calibre 3235, dial up, 300 s, 48 kHz 16-bit mono, C-Media
USB timegrapher microphone, no clipping (2026-10-08). tg here is
jnarvaezp/tg-mod (tg 0.7 lineage) run offline over the same file, reading
every 100 ms from its 16 s window.

| Measure | tg | timegrapher |
| --- | --- | --- |
| Amplitude (lift angle 52°) | 232° (median) | 232° |
| Rate spread over the run | −0.2 to 49.3 s/d (5th–95th pct) | −0.9 to 49.0 s/d |
| Rate over time | — | correlation 0.998 with tg's series, mean difference −0.3 s/d |
| Beat error | 0.17 ms (median) | 0.23 ms from the unlock (0.08 ms from the drop) |

Beat error is compared below, with the bench watches.
Absolute rate is uncalibrated in both: neither corrects the sound card's
clock yet.

## Against tg on the bench watches

tg here is jnarvaezp/tg-mod built with lift angle 55° (the value for
32xx movements), run offline on the same files. Medians of window values;
rate is on the uncalibrated sound-card clock.

| Recording | Amplitude tg | Amplitude timegrapher | Rate tg / timegrapher (s/d) |
| --- | --- | --- | --- |
| Dandong 3235, dial up, 30 min | 286° | 288° | +3.8 / +3.6 |
| Rolex 3235, 2 h take, 30 min of it (segments 1, 2, 7) | 251° | 250° | +43.1 / +43.1 |
| Rolex 3235, 5 min | 246° | 245° | +35.6 / +34.1 |

On the Dandong, sound 1 on one side is only 4–5% as loud as the drop.
A threshold of 5% of the drop's height missed it and timed the unlock from
sound 2, reading 297°. The unlock is now the first sustained rise above the
larger of 2% of the drop or 4 noise SDs, timed at half its own height.

## Beat error from the unlock

tg and commercial timegraphers measure beat error from the unlock (sound 1).
timegrapher's beat times sit near the drop (sound 3), so the fit gives the
drop's beat error; the two differ by the difference between the two sides'
unlock-to-drop times. timegrapher now reports both, with the unlock value
first: the drop's beat error in each 2 s amplitude window, moved by how
far each side's unlock edge sits from its beat times on that window's
templates, and the median over the windows. tg values are medians of its
16 s windows; timegrapher's are from `analyze` on each segment. Lift angle
does not enter.

| Recording, segment | tg (unlock) | timegrapher from the unlock | timegrapher from the drop |
| --- | --- | --- | --- |
| Dandong 3235, 01 | 0.19 ms | 0.20 ms | 0.44 ms |
| Dandong 3235, 02 | 0.12 ms | 0.25 ms | 0.29 ms |
| Dandong 3235, 03 | 0.14 ms | 0.29 ms | 0.34 ms |
| Rolex 3235 (2 h take), 01 | 0.29 ms | 0.29 ms | 0.13 ms |
| Rolex 3235 (2 h take), 02 | 0.27 ms | 0.25 ms | 0.09 ms |
| Rolex 3235 (2 h take), 07 | 0.36 ms | 0.32 ms | 0.12 ms |
| Rolex 3235, 5 min | 0.17 ms | 0.23 ms | 0.08 ms |
| Daytona (Dandong 4130), 01 | 0.86 ms | 0.21 ms | 0.02 ms |
| Daytona (Dandong 4130), 02 | 0.88 ms | 0.31 ms | 0.04 ms |
| Daytona (Dandong 4130), 03 | 0.87 ms | 0.30 ms | 0.05 ms |

On the Rolex the unlock value agrees with tg to within 0.04 ms on the 2 h
take and reads 0.06 ms high on the 5 min recording. On the Dandong
it is rough: within 0.01 ms on segment 01 but 0.13–0.15 ms high on 02 and
03. The Daytona does not agree, and the reason is the landmark, not a
missed sound. In tg's own trace the two sides' unlock-to-drop times differ
by 0.83–0.85 ms (median per window); timegrapher's differ by about 0.4 ms,
with the same mean, which is why the amplitudes agree (321° against
324°). On the Daytona sound 1 has a different shape on each side. On the
whole-recording templates both sides' sound 1 starts about 7.5 ms before
the drop, but on one side it rises in 0.5 ms to a sharp peak, and on the
other it rises slowly to a broad peak about 1.2 ms after its start.
timegrapher times the unlock at half of sound 1's height on its rising
edge; tg times it at the first peak after its threshold, on a waveform
smoothed over 1 ms, and so reads the late peak on the broad side. Measured
peak to peak on timegrapher's 2 s templates, the sides differ by about
0.65 ms. Which landmark a commercial timegrapher uses on a beat like this
is not known here; the bench No. 1900 read 0.3 ms on this watch, close to
timegrapher's value, but that is a single photo.

Single windows are noisy on the two Dandong-built watches: the unlock
correction scatters by 1–1.7 ms (SD) from window to window, against
0.1 ms on the Rolex, so only the median is meaningful there. `long` on a
whole 30 min folder gives 0.25 ms on the Dandong 3235 and 0.32 ms on the
Daytona.

## Beat shape

`crates/timegrapher-core/tests/shape.rs` checks `timegrapher shape` on
synthetic beats whose three sounds are placed and scaled on purpose:

| Check | Tolerance |
| --- | --- |
| 1→2 and 2→3 intervals (impulse at 35% and 60% of unlock-to-drop) | ±0.1 ms |
| Level ratios 1:3 and 2:3 | ±0.03 |
| An extra sound 4 ms after the drop on every other beat | found on that side only, ±0.1 ms |
| Impulse 0.2 ms after the unlock | reported as not separable (no sound 2) |
