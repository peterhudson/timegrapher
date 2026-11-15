use super::*;
use crate::synth::{generate, SynthConfig};

/// A fresh folder for one test.
fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-beat-cache-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A take of two sound files in `dir`, last changed an hour ago (so not
/// still being written), with clipped beats and a jump in the beats.
fn take(dir: &Path) -> Vec<PathBuf> {
    let mut a = generate(
        &SynthConfig {
            duration_s: 180.0,
            ..Default::default()
        },
        |t| 270.0 + 10.0 * (t / 50.0).sin(),
        |_| 0.3,
    );
    let fs_hz = a.sample_rate as f64;
    for (i, v) in a.samples.iter_mut().enumerate() {
        if (100.0..110.0).contains(&(i as f64 / fs_hz)) {
            *v = (*v * 40.0).clamp(-1.0, 1.0);
        }
    }
    let cut = (60.0 * fs_hz) as usize;
    a.samples.drain(cut..cut + (2.3 * fs_hz) as usize);
    let half = a.samples.len() / 2;
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    [
        ("take-001.wav", 0..half),
        ("take-002.wav", half..a.samples.len()),
    ]
    .into_iter()
    .map(|(name, part)| {
        let p = dir.join(name);
        let part = crate::audio::Audio {
            samples: a.samples[part].to_vec(),
            sample_rate: a.sample_rate,
        };
        crate::audio::write_wav(&p, &part).unwrap();
        set_changed(&p, hour_ago);
        p
    })
    .collect()
}

fn set_changed(p: &Path, t: SystemTime) {
    fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

/// The beats of a take come back from their copy exactly as the sound
/// gave them, every field to the bit: beats, amplitude windows, clipping,
/// jumps, the beat-sound snippets and the loudest sample.
#[test]
fn a_kept_copy_reads_back_exactly_as_the_sound_gave_it() {
    let dir = folder("exact");
    let files = take(&dir);
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let cfg = StreamConfig::default();
    let (log, peak) = crate::stream::analyze_files_peak(&paths, &cfg, |_| {}).unwrap();
    assert!(!log.jumps.is_empty(), "no jump");
    assert!(!log.clipped_beats.is_empty(), "no clipping");
    assert!(!log.snippets.is_empty(), "no snippets");
    assert!(log.amplitude_windows.iter().any(|w| w.even_deg.is_none()));
    let cache = dir.join("cache");
    write(&cache, &paths, &cfg, &log, peak).unwrap();
    let (back, back_peak) = read(&cache, &paths, &cfg).expect("the copy reads");
    assert_eq!(encode(&back, back_peak), encode(&log, peak));
    assert_eq!(format!("{back:?}"), format!("{log:?}"));
    let _ = fs::remove_dir_all(&dir);
}

/// A take read with the lock-on template keeps that from its copy, so a
/// take reopened from the cache still carries its caveat (#1503); a copy
/// with any other byte there is no copy.
#[test]
fn a_kept_lock_on_template_reads_back_from_the_copy() {
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let mut log =
        crate::stream::analyze_samples(&audio.samples, audio.sample_rate, &StreamConfig::default());
    for kept in [true, false] {
        log.lock_template_kept = kept;
        let bytes = encode(&log, 0.5);
        let (back, _) = decode(&bytes).expect("the copy reads");
        assert_eq!(back.lock_template_kept, kept);
        assert_eq!(encode(&back, 0.5), bytes);
    }
    let mut bytes = encode(&log, 0.5);
    // The flag's byte: after the layout's name, the rate and beat rate,
    // the length and lift, the peak and the clipped samples.
    bytes[16 + 4 + 4 + 16 + 4 + 8] = 2;
    assert!(decode(&bytes).is_none());
}

/// A copy is only read back for the same sound, engine settings and
/// layout: a changed file, a changed setting or a damaged copy reads as
/// no copy, never as other beats.
#[test]
fn a_copy_is_read_only_for_the_same_sound_and_settings() {
    let dir = folder("stale");
    let files = take(&dir);
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let cfg = StreamConfig::default();
    let (log, peak) = crate::stream::analyze_files_peak(&paths, &cfg, |_| {}).unwrap();
    let cache = dir.join("cache");
    write(&cache, &paths, &cfg, &log, peak).unwrap();
    assert!(read(&cache, &paths, &cfg).is_some());
    // Another setting.
    let mut other = cfg.clone();
    other.analysis.amplitude.lift_deg += 1.0;
    assert!(read(&cache, &paths, &other).is_none());
    // The files in another order.
    let swapped = [paths[1], paths[0]];
    assert!(read(&cache, &swapped, &cfg).is_none());
    // A file changed: the same length, a new time.
    set_changed(&files[1], SystemTime::now() - Duration::from_secs(1800));
    assert!(read(&cache, &paths, &cfg).is_none());
    set_changed(&files[1], SystemTime::now() - Duration::from_secs(3600));
    write(&cache, &paths, &cfg, &log, peak).unwrap();
    assert!(read(&cache, &paths, &cfg).is_some());
    // A byte of sound changed inside the first 64 KiB, the length and
    // the time the same.
    let changed_at = |f: &Path| fs::metadata(f).unwrap().modified().unwrap();
    let was = changed_at(&files[0]);
    let mut bytes = fs::read(&files[0]).unwrap();
    bytes[1000] ^= 0x01;
    fs::write(&files[0], &bytes).unwrap();
    set_changed(&files[0], was);
    assert!(read(&cache, &paths, &cfg).is_none());
    bytes[1000] ^= 0x01;
    fs::write(&files[0], &bytes).unwrap();
    set_changed(&files[0], was);
    assert!(read(&cache, &paths, &cfg).is_some());
    // A byte added past the first 64 KiB, the time the same.
    let was = changed_at(&files[1]);
    let mut bytes = fs::read(&files[1]).unwrap();
    bytes.push(0);
    fs::write(&files[1], &bytes).unwrap();
    set_changed(&files[1], was);
    assert!(read(&cache, &paths, &cfg).is_none());
    bytes.pop();
    fs::write(&files[1], &bytes).unwrap();
    set_changed(&files[1], was);
    assert!(read(&cache, &paths, &cfg).is_some());
    // The copy damaged.
    let kept = cache.join(key(&paths, &cfg).unwrap());
    let mut raw = fs::read(&kept).unwrap();
    let mid = raw.len() / 2;
    raw[mid] ^= 0x55;
    fs::write(&kept, &raw).unwrap();
    assert!(read(&cache, &paths, &cfg).is_none());
    // Cut short, after the gzip, it reads as nothing too.
    let bytes = encode(&log, peak);
    assert!(decode(&bytes[..bytes.len() - 1]).is_none());
    assert!(decode(&[bytes.clone(), vec![0]].concat()).is_none());
    let _ = fs::remove_dir_all(&dir);
}

/// A copy of another take is never read as this one (#1326): not when
/// its files have the same names, lengths, times and starts in another
/// folder, nor when they differ only near the end or the middle.
#[test]
fn a_copy_is_read_only_for_the_same_files() {
    let dir = folder("same-files");
    let files = take(&dir);
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let cfg = StreamConfig::default();
    let (log, peak) = crate::stream::analyze_files_peak(&paths, &cfg, |_| {}).unwrap();
    let cache = dir.join("cache");
    write(&cache, &paths, &cfg, &log, peak).unwrap();
    assert!(read(&cache, &paths, &cfg).is_some());
    // The same files, byte for byte and time for time, in another folder.
    let elsewhere = dir.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let copies: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            let c = elsewhere.join(f.file_name().unwrap());
            fs::copy(f, &c).unwrap();
            set_changed(&c, fs::metadata(f).unwrap().modified().unwrap());
            c
        })
        .collect();
    let copied: Vec<&Path> = copies.iter().map(|f| f.as_path()).collect();
    assert!(read(&cache, &copied, &cfg).is_none());
    // A file much longer than the spans read, so the whole of it isn't.
    let len = fs::metadata(&files[1]).unwrap().len();
    assert!(len > 4 * SPANS * HEAD_BYTES as u64, "the take is too short");
    let mid = (SPANS / 2 * (len - HEAD_BYTES as u64) / (SPANS - 1)) as usize + 7;
    for at in [len as usize - 3, mid] {
        let was = fs::metadata(&files[1]).unwrap().modified().unwrap();
        let mut bytes = fs::read(&files[1]).unwrap();
        bytes[at] ^= 0x01;
        fs::write(&files[1], &bytes).unwrap();
        set_changed(&files[1], was);
        assert!(
            read(&cache, &paths, &cfg).is_none(),
            "a byte at {at} of {len}"
        );
        bytes[at] ^= 0x01;
        fs::write(&files[1], &bytes).unwrap();
        set_changed(&files[1], was);
        assert!(read(&cache, &paths, &cfg).is_some());
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A file no longer than the spans is read whole into the key: a byte
/// changed past its first 64 KiB, its length and time the same, is seen.
#[test]
fn a_short_file_is_keyed_on_every_byte() {
    let dir = folder("short-file");
    let f = dir.join("short.wav");
    let len = (SPANS as usize - 1) * HEAD_BYTES;
    let mut bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    fs::write(&f, &bytes).unwrap();
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    set_changed(&f, hour_ago);
    let cfg = StreamConfig::default();
    let was = key(&[f.as_path()], &cfg).unwrap();
    for at in [HEAD_BYTES + 5, len / 2 + 3, len - 1] {
        bytes[at] ^= 0x01;
        fs::write(&f, &bytes).unwrap();
        set_changed(&f, hour_ago);
        assert_ne!(
            key(&[f.as_path()], &cfg).unwrap(),
            was,
            "a byte at {at} of {len}"
        );
        bytes[at] ^= 0x01;
        fs::write(&f, &bytes).unwrap();
        set_changed(&f, hour_ago);
        assert_eq!(key(&[f.as_path()], &cfg).unwrap(), was);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The length is in the key on its own (#1350): two long files of the
/// same bytes everywhere the spans read (all zeros), at one place and time,
/// differ only in length, and each has its own key.
#[test]
fn a_file_of_another_length_has_another_key() {
    let dir = folder("length");
    let f = dir.join("zeros.wav");
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    let cfg = StreamConfig::default();
    let mut keys = Vec::new();
    for len in [
        (SPANS as usize + 4) * HEAD_BYTES,
        (SPANS as usize + 9) * HEAD_BYTES,
    ] {
        fs::write(&f, vec![0u8; len]).unwrap();
        set_changed(&f, hour_ago);
        keys.push(key(&[f.as_path()], &cfg).unwrap());
    }
    assert_ne!(keys[0], keys[1]);
    let _ = fs::remove_dir_all(&dir);
}

/// No copy is kept of a take whose files changed in the last two minutes:
/// it may still be recording.
#[test]
fn no_copy_is_kept_of_a_take_still_being_written() {
    let dir = folder("writing");
    let files = take(&dir);
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let cfg = StreamConfig::default();
    let (log, peak) = crate::stream::analyze_files_peak(&paths, &cfg, |_| {}).unwrap();
    set_changed(&files[1], SystemTime::now());
    let cache = dir.join("cache");
    write(&cache, &paths, &cfg, &log, peak).unwrap();
    assert!(read(&cache, &paths, &cfg).is_none());
    assert!(!cache.exists() || fs::read_dir(&cache).unwrap().next().is_none());
    let _ = fs::remove_dir_all(&dir);
}

/// The folder is held to its size by removing the copies used longest
/// ago, and leaves other files alone.
#[test]
fn the_folder_is_held_to_its_size_by_the_least_recently_used() {
    let dir = folder("size");
    let at = |s: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + s);
    for (name, used) in [("old", 1), ("mid", 2), ("new", 3)] {
        let p = dir.join(name);
        fs::write(&p, vec![0u8; 100]).unwrap();
        set_changed(&p, at(used));
    }
    fs::write(dir.join("x.part"), vec![0u8; 500]).unwrap();
    hold_to(&dir, 200);
    let mut left: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["mid", "new", "x.part"]);
    let _ = fs::remove_dir_all(&dir);
}

/// A window's culled sides' edges are kept in the copy and read back
/// (#1333).
#[test]
fn a_copy_keeps_the_culled_sides_edges() {
    let dir = folder("culled");
    let files = take(&dir);
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let cfg = StreamConfig::default();
    let (mut log, peak) = crate::stream::analyze_files_peak(&paths, &cfg, |_| {}).unwrap();
    log.amplitude_windows[0].set_culled([Some([-21.5, 0.25]), None]);
    log.amplitude_windows[2].set_culled([None, Some([-4.75, 0.5])]);
    let (back, _) = decode(&encode(&log, peak)).expect("the copy reads");
    for (b, w) in back.amplitude_windows.iter().zip(&log.amplitude_windows) {
        assert_eq!(b.culled(), w.culled());
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A window's sides withheld as too faint to read are kept in the copy and
/// read back, so a take opened from its copy names them as the first read
/// did (#1738).
#[test]
fn a_copy_keeps_the_sides_withheld_as_faint() {
    let dir = folder("faint");
    let files = take(&dir);
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let cfg = StreamConfig::default();
    let (mut log, peak) = crate::stream::analyze_files_peak(&paths, &cfg, |_| {}).unwrap();
    log.amplitude_windows[0].edit_extra(|x| x.faint_withheld = [true, false]);
    log.amplitude_windows[1].edit_extra(|x| x.faint_withheld = [false, true]);
    log.amplitude_windows[2].edit_extra(|x| x.faint_withheld = [true, true]);
    let (back, _) = decode(&encode(&log, peak)).expect("the copy reads");
    for (b, w) in back.amplitude_windows.iter().zip(&log.amplitude_windows) {
        assert_eq!(b.faint_withheld(), w.faint_withheld());
    }
    let _ = fs::remove_dir_all(&dir);
}
