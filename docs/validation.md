# Validation

## Synthetic recordings

`cargo test` generates recordings with known values and checks that they
come back out (`crates/timegrapher-core/tests/synthetic.rs`):

| Check | Tolerance |
| --- | --- |
| Beat rate guessed at 18,000, 21,600, 28,800 and 36,000 bph | exact |
| Rate (−7 s/d set) | ±0.3 s/d |
| Beat error (0.6 ms set) | ±0.05 ms |
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
| Beat error | 0.17 ms (median) | 0.08 ms |

The beat-error difference is not resolved yet: tg reports the median of
its 16 s windows, while timegrapher fits the whole run at once.
Absolute rate is uncalibrated in both: neither corrects the sound card's
clock yet.

## Beat shape

`crates/timegrapher-core/tests/shape.rs` checks `timegrapher shape` on
synthetic beats whose three sounds are placed and scaled on purpose:

| Check | Tolerance |
| --- | --- |
| 1→2 and 2→3 intervals (impulse at 35% and 60% of unlock-to-drop) | ±0.1 ms |
| Level ratios 1:3 and 2:3 | ±0.03 |
| An extra sound 4 ms after the drop on every other beat | found on that side only, ±0.1 ms |
| Impulse 0.2 ms after the unlock | reported as not separable (no sound 2) |
