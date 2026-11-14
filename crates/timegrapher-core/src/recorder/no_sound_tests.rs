//! A recording that writes none of its sound (#1514): its sound goes to
//! the analysis as it comes, and the folder keeps the rest.

use super::*;

fn info() -> SessionInfo {
    SessionInfo {
        watch: "Depth".into(),
        position: "DU".into(),
        lift_deg: 52.0,
        bph: None,
        device: "test".into(),
        sample_rate: 8000,
        bits: 16,
        software: "test".into(),
    }
}

fn block(n: usize, at_s: u64) -> Block {
    Block {
        samples: vec![0.25; n],
        at: UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000 + at_s),
    }
}

/// Without its sound a recording writes no sound file, through a pause
/// too, but counts its length, logs its clock, lists its break and keeps
/// its session file; it is never saved with its sound. Once any sound is
/// written it can't go without.
#[test]
fn a_recording_without_its_sound_keeps_all_but_the_sound() {
    let parent = std::env::temp_dir().join(format!("tg-rec-nosound-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    let mut r = Recorder::start(&parent, info()).unwrap();
    assert!(r.keeps_sound());
    r.without_sound().unwrap();
    assert!(!r.keeps_sound());
    assert_eq!(r.bytes_per_frame(), 2);
    for k in 0..30 {
        r.write(&block(8000, k)).unwrap();
    }
    r.pause().unwrap();
    r.resume("test");
    for k in 40..50 {
        r.write(&block(8000, k)).unwrap();
    }
    r.flush().unwrap();
    assert_eq!(r.duration_s(), 40.0);
    assert_eq!(r.breaks().len(), 1);
    assert_eq!(r.breaks()[0].audio_s, 30.0);
    assert!(r.segment_paths().is_empty());
    let names: Vec<String> = fs::read_dir(r.dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !is_sound(n)), "{names:?}");
    let clock = fs::read_to_string(r.dir().join(take::CLOCK_LOG)).unwrap();
    assert!(clock.lines().count() > 4, "{clock}");
    let e = r.save_copy(&parent, "", None).unwrap_err().to_string();
    assert!(e.contains("keeps no sound"), "{e}");
    let dir = r.finish(None).unwrap();
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join(take::RECORD)).unwrap()).unwrap();
    assert_eq!(record["segments"], serde_json::json!([]));
    let mut r = Recorder::start(&parent, info()).unwrap();
    r.write(&block(10, 0)).unwrap();
    assert!(r.without_sound().is_err());
    assert!(r.keeps_sound());
    drop(r);
    let _ = fs::remove_dir_all(&parent);
}
