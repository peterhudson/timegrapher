//! The beats' sound a take kept as its analysis keeps for the watch
//! verdict ([`SOUND`], #2069): a short take reads, kept, as it read with
//! its sound.

use super::*;
use crate::live::{LiveAnalyzer, LiveConfig, LiveReading};
use crate::synth::{generate, SynthConfig};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-kept-sound-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A clean watch of `seconds`, as a take's sound file in `dir`.
fn watch(dir: &Path, seconds: f64) -> PathBuf {
    let audio = generate(
        &SynthConfig {
            duration_s: seconds,
            rate_s_per_day: -22.0,
            beat_error_ms: 0.14,
            seed: 2069,
            ..Default::default()
        },
        |_| 204.0,
        |_| 0.0,
    );
    let path = dir.join("take_part01-001.wav");
    audio::write_wav(&path, &audio).unwrap();
    path
}

/// The take in `dir` (sound `wav`) as it opens with its sound, and as it
/// opens once kept as its analysis alone.
fn with_sound_and_kept(dir: &Path, wav: &Path) -> (BeatLog, BeatLog) {
    let cfg = StreamConfig::default();
    let sound = stream::analyze_files(&[wav], &cfg, |_| {}).unwrap();
    let software = serde_json::json!({"name": "timegrapher", "version": "test"});
    let s = measure(&[wav], &cfg, &[], software, |_| {}).unwrap();
    let kept = dir.join("kept");
    write(&kept, &s).unwrap();
    (sound, read(&kept).unwrap().log)
}

/// Amplitude and beat error as the cards show them (to 1° and 0.01 ms),
/// and whether there is a rate.
fn shown(r: &LiveReading) -> (bool, [Option<i64>; 2]) {
    let at = |v: Option<f64>, step: f64| v.map(|v| (v / step).round() as i64);
    (
        r.rate_s_per_day.is_some(),
        [at(r.amplitude_deg, 1.0), at(r.beat_error_ms, 0.01)],
    )
}

/// The most a rate fitted over `span_s` of evenly spread beats moves when
/// each beat time moves by up to e = 0.5 µs, as [`BEATS`] keeps them: a
/// least-squares slope moves by at most 3e over the span, in s/d. Over 1 s
/// that is 0.13 s/d, so a card can show 0.1 s/d apart; over 5 s, 0.03.
fn rate_margin(span_s: f64) -> f64 {
    86_400.0 * 3.0 * 0.5e-6 / span_s.max(1.0) + 1e-9
}

/// The verdict and the figures shown of `kept` and `sound` agree at every
/// half second from `from_s` to the end, at 10, 30 and 60 s averages;
/// the moments with figures.
fn same_readings(sound: &LiveAnalyzer, kept: &LiveAnalyzer, from_s: f64) -> usize {
    let mut with_figures = 0;
    let mut t = from_s;
    while t <= sound.duration_s() + 1e-9 {
        for average_s in [10.0, 30.0, 60.0] {
            let (a, b) = (
                sound.reading_at(t, average_s),
                kept.reading_at(t, average_s),
            );
            let at = format!("{t:.1} s, {average_s} s average");
            assert_eq!(b.watch_heard, a.watch_heard, "{at}: heard");
            assert_eq!(b.listening, a.listening, "{at}: waiting");
            assert_eq!(shown(&b), shown(&a), "{at}: figures");
            if let (Some(x), Some(y)) = (a.rate_s_per_day, b.rate_s_per_day) {
                let m = rate_margin(a.span_s);
                assert!((x - y).abs() <= m, "{at}: rate {x} and {y}, over {m}");
            }
            with_figures += usize::from(b.rate_s_per_day.is_some());
        }
        t += 0.5;
    }
    with_figures
}

/// The beats' sound comes back exactly: the same windows, in the same
/// order, every value to the bit, each at its beat as kept.
fn same_sound(sound: &BeatLog, kept: &BeatLog) {
    assert!(!sound.snippets.is_empty());
    assert_eq!(kept.snippets.len(), sound.snippets.len());
    for (a, b) in sound.snippets.iter().zip(&kept.snippets) {
        assert!((a.0 - b.0).abs() <= 0.5e-6 + 1e-9, "{} {}", a.0, b.0);
        let bits = |w: &[f32]| w.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&b.1), bits(&a.1), "the window at {} s", a.0);
    }
}

/// #2069: a short take, kept as its analysis alone, reads as with its
/// sound, as a whole and at every moment: under 10 s both wait, with no
/// figures (Norm on #2333), and from 10 s both read. Before #2069 the
/// kept take waited where the one with its sound read.
#[test]
fn a_short_take_kept_reads_as_with_its_sound() {
    let dir = scratch("short");
    let wav = watch(&dir, 12.0);
    let (sound, kept) = with_sound_and_kept(&dir, &wav);
    same_sound(&sound, &kept);
    // The take as a whole (its Watches row).
    let whole = |l: &BeatLog| session::measure(l, None, 0.0, l.duration_s);
    let (a, b) = (whole(&sound), whole(&kept));
    assert!(
        a.watch_heard && a.rate_s_per_day.is_some(),
        "with its sound"
    );
    assert!(b.watch_heard, "kept: {:?}", b.caveats);
    let round = |v: Option<f64>, step: f64| v.map(|v| (v / step).round() as i64);
    for (what, x, y, step) in [
        ("rate", a.rate_s_per_day, b.rate_s_per_day, 0.1),
        ("amplitude", a.amplitude_deg, b.amplitude_deg, 1.0),
        ("beat error", a.beat_error_ms, b.beat_error_ms, 0.01),
    ] {
        assert_eq!(round(y, step), round(x, step), "{what}");
    }
    // Opened (the cards), every moment.
    let open = |l: &BeatLog| LiveAnalyzer::from_log(l.clone(), LiveConfig::default());
    let (a, b) = (open(&sound), open(&kept));
    assert!(same_readings(&a, &b, 1.0) > 0, "no moment with figures");
    for t in [1.0, 5.0, 9.5] {
        let r = b.reading_at(t, 10.0);
        assert!(
            r.rate_s_per_day.is_none() && r.listening.is_some(),
            "kept, at {t} s: {r:?}"
        );
    }
    let end = b.reading(12.0);
    assert!(
        end.rate_s_per_day.is_some() && end.amplitude_deg.is_some(),
        "kept, at the end: {:?}",
        end.listening
    );
    let _ = fs::remove_dir_all(&dir);
}

/// After a break, kept, each reading is the one with its sound: under
/// 10 s after the break both wait (Norm on #2333), then both read.
#[test]
fn under_ten_seconds_after_a_break_reads_as_with_its_sound() {
    let dir = scratch("break");
    let wav = watch(&dir, 30.0);
    let (sound, kept) = with_sound_and_kept(&dir, &wav);
    let open = |l: &BeatLog| {
        let mut a = LiveAnalyzer::from_log(l.clone(), LiveConfig::default());
        a.set_breaks(vec![16.0]);
        a
    };
    let (a, b) = (open(&sound), open(&kept));
    assert!(same_readings(&a, &b, 16.5) > 0, "no moment with figures");
    let waiting = b.reading_at(23.0, 10.0);
    assert!(waiting.rate_s_per_day.is_none() && waiting.listening.is_some());
    assert!(b.reading_at(29.0, 10.0).rate_s_per_day.is_some());
    let _ = fs::remove_dir_all(&dir);
}

/// The file is bounded whatever the take's length: at most
/// [`crate::sound_sample::SOUND_SAMPLE_BEATS`] windows of one template's
/// length, and a take long enough fills the budget.
#[test]
fn the_beat_sound_is_bounded_whatever_the_take_length() {
    let dir = scratch("bound");
    let wav = watch(&dir, 90.0);
    let (sound, kept) = with_sound_and_kept(&dir, &wav);
    same_sound(&sound, &kept);
    let budget = crate::sound_sample::SOUND_SAMPLE_BEATS;
    let n = sound.snippets.len();
    assert!(n > budget / 2 && n <= budget, "{n} windows");
    let len = crate::profile::template_len(
        sound.sample_rate as f64,
        crate::amplitude::UnlockWindow::SHORT,
    );
    let bin = sound_bin(&sound);
    assert!(bin.len() <= sound_bin_max_bytes(len), "{} bytes", bin.len());
    let _ = fs::remove_dir_all(&dir);
}

/// The most [`sound_bin`] can be with windows of `len` values.
pub(crate) fn sound_bin_max_bytes(len: usize) -> usize {
    12 + crate::sound_sample::SOUND_SAMPLE_BEATS * (8 + 4 * len)
}

/// A beat sound file that is missing, cut short, of a later version, out
/// of order or past the beats is read as none, never a guess: the take
/// still opens, and its short stretches wait for a verdict.
#[test]
fn a_beat_sound_that_cant_be_read_is_none() {
    let dir = scratch("bad");
    let wav = watch(&dir, 5.0);
    let (_, kept) = with_sound_and_kept(&dir, &wav);
    let good = sound_bin(&kept);
    let path = dir.join("kept").join(DIR).join(SOUND);
    let mut later = good.clone();
    later[7] = 2;
    let mut past = good.clone();
    past[12..16].copy_from_slice(&(kept.beats.len() as u32).to_le_bytes());
    let mut swapped = good.clone();
    let row = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let len0 = row(&good, 16) as usize;
    let second = 20 + 4 * len0;
    let (r0, r1) = (row(&good, 12), row(&good, second));
    swapped[12..16].copy_from_slice(&r1.to_le_bytes());
    swapped[second..second + 4].copy_from_slice(&r0.to_le_bytes());
    let mut nan = good.clone();
    nan[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
    let mut longer = good.clone();
    longer.push(0);
    for (what, bytes) in [
        ("cut short", Some(good[..good.len() - 1].to_vec())),
        ("a later version", Some(later)),
        ("past the beats", Some(past)),
        ("out of order", Some(swapped)),
        ("not a number", Some(nan)),
        ("running on", Some(longer)),
        ("not gzip", None),
        ("missing", None),
    ] {
        match (what, bytes) {
            ("missing", _) => fs::remove_file(&path).unwrap(),
            (_, Some(b)) => fs::write(&path, gzip(&b).unwrap()).unwrap(),
            (_, None) => fs::write(&path, b"TGSOUND\x01 not gzip").unwrap(),
        }
        if let Ok(raw) = fs::read(&path) {
            let unzipped = {
                let mut out = Vec::new();
                GzDecoder::new(raw.as_slice())
                    .read_to_end(&mut out)
                    .map(|_| out)
            };
            if let Ok(u) = unzipped {
                assert!(parse_sound(&u, &kept.beats).is_err(), "{what}");
            }
        }
        let back = read(&dir.join("kept")).unwrap();
        assert!(back.log.snippets.is_empty(), "{what}");
        assert_eq!(back.log.beats.len(), kept.beats.len(), "{what}");
        let r = LiveAnalyzer::from_log(back.log, LiveConfig::default()).reading(10.0);
        assert!(
            r.rate_s_per_day.is_none() && r.listening.is_some(),
            "{what}: {:?}",
            r.listening
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
