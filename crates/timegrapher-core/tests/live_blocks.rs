//! Live reads the same sound the same way whatever size of block it comes
//! in (#1720): a microphone sends 100 ms blocks under PulseAudio, a file
//! read and the tests 20 ms ones.

use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
use timegrapher_core::synth::{generate, SynthConfig};

/// The beats and amplitude windows `samples` give pushed in `block`s.
fn read_in(samples: &[f32], rate: u32, block: usize) -> LiveAnalyzer {
    let mut a = LiveAnalyzer::new(rate, LiveConfig::default());
    for b in samples.chunks(block) {
        a.push(b);
    }
    a
}

/// Half a minute of a noisy watch pushed in 20 ms, 100 ms and odd-sized blocks
/// finds the same first template, so the same beats at the same times and
/// the same amplitude in every window. Before, the passes ran at the end
/// of whichever block reached them, so the template came from another
/// stretch of sound and the beats sat up to a millisecond apart: on one
/// real take, 284° live against 336° reopened.
#[test]
fn live_reads_the_same_whatever_the_block_size() {
    let audio = generate(
        &SynthConfig {
            duration_s: 30.0,
            snr_db: 20.0,
            seed: 1720,
            ..Default::default()
        },
        |t| 300.0 + 10.0 * (t / 7.0).sin(),
        |_| 0.0,
    );
    let rate = audio.sample_rate;
    let first = read_in(&audio.samples, rate, 960);
    assert!(first.beats().len() > 200, "{}", first.beats().len());
    assert!(!first.amplitude_windows().is_empty());
    for block in [4_800, 1_031, 48_000] {
        let other = read_in(&audio.samples, rate, block);
        // Bit for bit: neither type compares, so their printouts do.
        let seen = |a: &LiveAnalyzer| format!("{:?}", (a.beats(), a.amplitude_windows()));
        assert!(
            seen(&other) == seen(&first),
            "beats or amplitude in {block} blocks"
        );
        assert_eq!(other.jumps(), first.jumps(), "jumps in {block} blocks");
    }
}
