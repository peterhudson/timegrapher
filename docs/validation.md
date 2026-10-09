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
| Sides that differ in shape (impulse louder than the drop on one, unlock nearly as loud on the other, levels ±25% beat to beat) | beat error 0.4 ms ±0.05, jitter under 50 µs, each side's amplitude ±8° |

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
| Dandong 3235, 01 | 0.19 ms | 0.19 ms | 0.45 ms |
| Dandong 3235, 02 | 0.12 ms | 0.24 ms | 0.35 ms |
| Dandong 3235, 03 | 0.14 ms | 0.15 ms | 0.77 ms |
| Rolex 3235 (2 h take), 01 | 0.29 ms | 0.30 ms | 0.08 ms |
| Rolex 3235 (2 h take), 02 | 0.27 ms | 0.26 ms | 0.06 ms |
| Rolex 3235 (2 h take), 07 | 0.36 ms | 0.31 ms | 0.08 ms |
| Rolex 3235, 5 min | 0.17 ms | 0.24 ms | 0.09 ms |
| Daytona (Dandong 4130), 01 | 0.86 ms | 0.36 ms | 0.05 ms |
| Daytona (Dandong 4130), 02 | 0.88 ms | 0.28 ms | 0.07 ms |
| Daytona (Dandong 4130), 03 | 0.87 ms | 0.28 ms | 0.06 ms |
| Patek 324 (Dandong clone), 01 | 0.41 ms | 0.49 ms | 0.32 ms |
| Patek 324 (Dandong clone), 02 | 0.39 ms | 0.46 ms | 0.35 ms |
| Patek 324 (Dandong clone), 03 | 0.43 ms | 0.45 ms | 0.34 ms |

On the Rolex the unlock value agrees with tg to within 0.05 ms on the 2 h
take and reads 0.07 ms high on the 5 min recording. On the Dandong
it is within 0.01 ms on segments 01 and 03 but 0.12 ms high on 02. On the
Patek 324 clone it reads 0.02–0.08 ms high. The Daytona does not agree, and the reason is the landmark, not a
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

Single windows are noisy on the Dandong-built watches. On the Dandong
3235 the unlock value scatters by about 0.55 ms (robust SD) from window
to window. On the Daytona and the Patek clone the scatter is 0.05–0.2 ms,
but 3–7% of windows are more than 1 ms off, because one side's unlock
edge is found several ms early. The Rolex scatters by 0.07 ms. Only the
median is meaningful on these watches. `long` on a whole 30 min folder
gives 0.19 ms on the Dandong 3235, 0.30 ms on the Daytona and 0.47 ms on
the Patek clone (tg 0.41 ms).

### Beats on the wrong sound: the Patek 324 clone

On this watch the two sides' beats differ in shape. On one side the
impulse (sound 2) is louder than the drop. On the other the unlock is
nearly as loud as the drop and the impulse is quiet. The levels also
wander from beat to beat. The first pass used to build its template from
each beat's loudest point, which was the impulse on some beats and the
unlock or the drop on others. The template was a blend. On segments 01
and 03 its reference point sat on one side's impulse, and on the other
side the correlation peaks moved between the unlock, the impulse and the
drop from beat to beat: 2.5 ms early, on time or 6 ms late. The
amplitude templates of that side were then blends too. On the side
timed at its impulse, the drop was outside the search for it. Most
windows had no amplitude on one side (288 and 291 of 299). The headline
beat error was the median of the few windows left, all of them wrong:
5.22 and 2.46 ms from the unlock, 1.63 and 0.14 ms from the drop, and
1089 and 416 µs of jitter. The amplitudes read 181/239° and 247/188°.

The first pass now holds each side on one sound: after the first two
beats, it takes the highest point within 1 ms of where that side's last
beat predicts the next one. It falls back to the whole search span only
when that span holds a peak more than twice as high. Each side's median
template is then built from those beats, and the side's beats are moved
onto its drop: the last sound that reaches 60% of the template's highest
peak with a dip below half height before it. The template for pass 2 is
built from the moved beats. A template rebuilt from pass 2's beats is
used for a final pass. The Patek clone now reads 0.49, 0.46 and 0.45 ms
(tg 0.41, 0.39 and 0.43 ms), 85, 86 and 71 µs of jitter, and
239/240°, 243/244° and 246/246° for the two sides (tg 238°, 244° and 246°).

On the other watches the beats line up more closely: Daytona jitter falls
from 292, 182 and 118 µs to 126, 102 and 92 µs, and Dandong 3235 segment 03
from 316 to 127 µs. On the Dandong 3235 one side's drop is broad, with
several peaks over 2.5 ms, and its beats now sit on the first one. On
segment 03 this moves the drop's beat error from 0.34 to 0.77 ms and that
side's amplitude from 300° to 308° (median 289° to 295° at 55°, against
tg's 286°). Segments 01 and 02 do not change by more than 1°.

## The drop edge on the bare ETA 2824

The ETA 2824-2 clone, taken out of its plastic holder (30 min dial up, and
10 min at a lower microphone gain), read 14–20° above tg at 50°. On this
movement the last sound before the drop's peak sits just under half the
drop's height and starts about 2 ms before it. The drop edge used to be the
first crossing of half height in the 2 ms before the peak, so in 28% of the
Tick side's 2 s windows a little noise on that earlier sound moved the edge
about 1 ms early and the window read 320–350°. The edge is now where the
rise into the peak crosses half height, found by walking back from the peak.

| Recording | tg | before | after |
| --- | --- | --- | --- |
| ETA 2824 bare, 01 / 02 / 03 (50°) | 279 / 283 / 281° | 297 / 299 / 293° | 290 / 290 / 288° |
| ETA 2824 bare, Mic 4/16 (50°) | 276° | 296° | 285° |
| Dandong 3235, 01 / 02 / 03 (55°) | 286° (whole) | 285 / 287 / 294° | 281 / 283 / 294° |
| Daytona, Patek 324 clone, Peacock SL1258 | | | within 1° |

Rate does not change. The remaining 7–11° on the bare ETA is a difference
of landmark: tg measures from the peak of sound 1 to the waveform's highest
point, and on this movement that point is about 1.1 ms after the drop's
rising edge (0.3–0.6 ms on the other watches). The ETA in its holder
remains unreliable either way.

## Beat shape

`crates/timegrapher-core/tests/shape.rs` checks `timegrapher shape` on
synthetic beats whose three sounds are placed and scaled on purpose:

| Check | Tolerance |
| --- | --- |
| 1→2 and 2→3 intervals (impulse at 35% and 60% of unlock-to-drop) | ±0.1 ms |
| Level ratios 1:3 and 2:3 | ±0.03 |
| An extra sound 4 ms after the drop on every other beat | found on that side only, ±0.1 ms |
| Impulse 0.2 ms after the unlock | reported as not separable (no sound 2) |
