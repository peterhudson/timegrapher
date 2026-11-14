//! A take's own files kept safe while it records: `session.json`
//! written whole (#1639), and Save Again never writing through a link
//! (#1640).

use super::*;

/// A note whose `session.json` can't be put in place returns the error,
/// leaves no half file beside it, and is kept in memory: the next write
/// that can, writes it.
#[test]
fn a_session_write_that_fails_leaves_no_half_file_and_keeps_the_note() {
    let parent = std::env::temp_dir().join(format!("tg-rec-session-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    let info = SessionInfo {
        watch: "Depth".into(),
        position: "DU".into(),
        lift_deg: 52.0,
        bph: None,
        device: "test".into(),
        sample_rate: 8000,
        bits: 16,
        software: "test".into(),
    };
    let mut r = Recorder::start(&parent, info).unwrap();
    let record = r.dir().join(take::RECORD);
    r.note("position DU").unwrap();
    let before = fs::read(&record).unwrap();
    assert!(!before.is_empty());
    // A folder in session.json's place: the move into place fails.
    fs::remove_file(&record).unwrap();
    fs::create_dir_all(record.join("held")).unwrap();
    assert!(r.note("position CL").is_err());
    assert!(record.join("held").is_dir(), "what was there stays");
    let names = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        v.sort();
        v
    };
    assert!(names(r.dir()).is_empty(), "{:?}", names(r.dir()));
    // Once it can be written, both notes are in it.
    fs::remove_dir_all(&record).unwrap();
    r.note("position CR").unwrap();
    let text = fs::read_to_string(&record).unwrap();
    for n in ["position DU", "position CL", "position CR"] {
        assert!(text.contains(n), "{n} in {text}");
    }
    drop(r);
    let _ = fs::remove_dir_all(&parent);
}

/// Save Again grows the copy's hour in place, but not through a link put
/// in its place: the save is refused and the WAV the link points at is
/// left as it was, where it had grown by the new sound (#1640).
#[cfg(unix)]
#[test]
fn save_again_writes_through_no_link_in_the_copy() {
    let (mut r, base) = super::tests::save_fixture("link");
    let save = |r: &mut Recorder| -> io::Result<PathBuf> {
        let copy = r.begin_stream_save(&base, "", None, &[])?.write()?;
        r.note_saved(&copy);
        Ok(copy.dir)
    };
    super::tests::append_second(&mut r, 2);
    let copy = save(&mut r).unwrap();
    let wav = copy.join("audio-001.wav");
    let other = base.join("other.wav");
    fs::copy(&wav, &other).unwrap();
    let before = fs::read(&other).unwrap();
    fs::remove_file(&wav).unwrap();
    std::os::unix::fs::symlink(&other, &wav).unwrap();
    super::tests::append_second(&mut r, 3);
    assert!(save(&mut r).is_err());
    assert_eq!(
        fs::read(&other).unwrap(),
        before,
        "the other WAV is untouched"
    );
    drop(r);
    let _ = fs::remove_dir_all(&base);
}
