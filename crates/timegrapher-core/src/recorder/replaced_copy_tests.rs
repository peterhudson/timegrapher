//! Save Again into a folder whose copy was deleted and another take put
//! in its place writes a new copy beside it and leaves that take alone
//! (#2437).

use super::super::tests::{append_second, save_fixture};
use super::super::SaveStep;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

/// Put another take in place of the copy at `dir`: its own sound and a
/// `session.json` naming another start.
fn replace_with_other_take(dir: &Path) {
    replace_with_sound(dir, b"other take");
}

/// [`replace_with_other_take`] with `sound` as its `audio-001.wav`.
fn replace_with_sound(dir: &Path, sound: &[u8]) {
    fs::remove_dir_all(dir).unwrap();
    fs::create_dir(dir).unwrap();
    fs::write(dir.join("audio-001.wav"), sound).unwrap();
    let other_session = br#"{"started_utc": "2026-11-26T05:30:40Z"}"#;
    fs::write(dir.join("session.json"), other_session).unwrap();
}

/// The refusal a save into `dir`, replaced since, gives.
fn refusal(dir: &Path) -> String {
    format!(
        "{} has changed since this take was saved there, so nothing was saved \
         into it; the next save makes a new copy beside it",
        dir.display()
    )
}

fn files(dir: &Path) -> BTreeMap<OsString, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().unwrap().is_file())
        .map(|e| (e.file_name(), fs::read(e.path()).unwrap()))
        .collect()
}

/// The probe from the issue: the copy saved to `A/<minute>` is deleted
/// outside the app and another take of that name copied in. Save Again
/// into `A` saves a new copy (` 2`), leaves that take's files as they
/// were, and goes on saving into the new copy.
#[test]
fn save_again_never_writes_into_a_take_put_in_place_of_its_copy() {
    let (mut r, base) = save_fixture("replaced-copy");
    let takes = base.join("A").join("takes");
    let name = "2026-11-26 05.30";
    let first = r.claim_first_copy(&takes, name).unwrap();
    assert_eq!(r.save_copy(&takes, "", None).unwrap(), first);
    replace_with_other_take(&first);
    let before = files(&first);
    append_second(&mut r, 2);
    let again = r.save_copy(&takes, "", None).unwrap();
    assert_eq!(again, takes.join(format!("{name} 2")));
    assert_eq!(files(&first), before);
    let sound = fs::read(again.join("audio-001.wav")).unwrap();
    assert!(sound.len() > 10);
    // Saved again, it goes on into the new copy and still leaves the
    // other take alone.
    append_second(&mut r, 3);
    assert_eq!(r.save_copy(&takes, "", None).unwrap(), again);
    assert!(fs::read(again.join("audio-001.wav")).unwrap().len() > sound.len());
    assert_eq!(files(&first), before);
    drop(r);
    let _ = fs::remove_dir_all(base);
}

/// A take put in place with no `session.json`, or one that can't be
/// read, is not this take's copy either.
#[test]
fn a_folder_with_no_readable_session_is_not_this_takes_copy() {
    for (case, session) in [("none", None), ("bad", Some(&b"not json"[..]))] {
        let (mut r, base) = save_fixture(&format!("replaced-copy-{case}"));
        let takes = base.join("takes");
        let first = r.save_copy(&takes, "", None).unwrap();
        fs::remove_dir_all(&first).unwrap();
        fs::create_dir(&first).unwrap();
        fs::write(first.join("audio-001.wav"), b"other take").unwrap();
        if let Some(text) = session {
            fs::write(first.join("session.json"), text).unwrap();
        }
        let before = files(&first);
        let again = r.save_copy(&takes, "", None).unwrap();
        assert_ne!(again, first);
        assert_eq!(files(&first), before);
        drop(r);
        let _ = fs::remove_dir_all(base);
    }
}

/// Its own copy, a copy deleted with nothing put back, and a copy emptied
/// are still saved into in place, as before.
#[test]
fn save_again_still_writes_into_its_own_copy() {
    let (mut r, base) = save_fixture("replaced-copy-own");
    let takes = base.join("takes");
    let first = r.save_copy(&takes, "", None).unwrap();
    append_second(&mut r, 2);
    assert_eq!(r.save_copy(&takes, "", None).unwrap(), first);
    fs::remove_dir_all(&first).unwrap();
    assert_eq!(r.save_copy(&takes, "", None).unwrap(), first);
    for e in fs::read_dir(&first).unwrap() {
        fs::remove_file(e.unwrap().path()).unwrap();
    }
    assert_eq!(r.save_copy(&takes, "", None).unwrap(), first);
    assert!(first.join("audio-001.wav").is_file());
    assert_eq!(fs::read_dir(&takes).unwrap().count(), 1);
    drop(r);
    let _ = fs::remove_dir_all(base);
}

/// The copy replaced after a save began, while it is copied on another
/// thread (Save Again, or a streamed save, which grows the open hour in
/// place where it is as long as the copy saved before: the other take's
/// sound is made that long here): the write is refused, says so, and
/// leaves the other take as it was. The next save makes a new copy
/// beside it.
#[test]
fn a_copy_replaced_while_saving_is_not_written_into() {
    for streamed in [false, true] {
        let (mut r, base) = save_fixture(&format!("replaced-copy-race-{streamed}"));
        let takes = base.join("takes");
        let first = r.save_copy(&takes, "", None).unwrap();
        append_second(&mut r, 2);
        let pending = match streamed {
            false => r.begin_save(&takes, "", None, &[]),
            true => r.begin_stream_save(&takes, "", None, &[]),
        }
        .unwrap();
        assert_eq!(pending.dest(), first);
        let len = fs::metadata(first.join("audio-001.wav")).unwrap().len();
        replace_with_sound(&first, &vec![0x55; len as usize]);
        let before = files(&first);
        let refused = pending.write().unwrap_err().to_string();
        assert_eq!(refused, refusal(&first));
        assert_eq!(files(&first), before);
        let again = r.save_copy(&takes, "", None).unwrap();
        assert_ne!(again, first);
        assert_eq!(files(&first), before);
        drop(r);
        let _ = fs::remove_dir_all(base);
    }
}

/// The copy replaced after its files are staged, just before they are put
/// in place: the save is refused there too, with the other take as it was.
#[test]
fn a_copy_replaced_after_staging_is_not_written_into() {
    let (mut r, base) = save_fixture("replaced-copy-staged");
    let takes = base.join("takes");
    let first = r.save_copy(&takes, "", None).unwrap();
    append_second(&mut r, 2);
    let pending = r.begin_save(&takes, "", None, &[]).unwrap();
    let mut replaced = false;
    let result = pending.write_using(|step, _| {
        if matches!(step, SaveStep::Stage) && !replaced {
            replaced = true;
            replace_with_other_take(&first);
        }
        Ok(())
    });
    assert!(replaced);
    let refused = result.unwrap_err().to_string();
    assert_eq!(refused, refusal(&first));
    assert_eq!(
        fs::read(first.join("audio-001.wav")).unwrap(),
        b"other take"
    );
    assert_eq!(fs::read_dir(&first).unwrap().count(), 2);
    drop(r);
    let _ = fs::remove_dir_all(base);
}

/// Something other than a folder put at the copy's name (a plain file):
/// Save Again makes a new copy beside it and leaves the file as it was.
#[test]
fn a_file_put_in_place_of_the_copy_is_left_alone() {
    let (mut r, base) = save_fixture("replaced-copy-file");
    let takes = base.join("takes");
    let first = r.save_copy(&takes, "", None).unwrap();
    fs::remove_dir_all(&first).unwrap();
    fs::write(&first, b"not a take").unwrap();
    let again = r.save_copy(&takes, "", None).unwrap();
    assert_ne!(again, first);
    assert!(again.join("audio-001.wav").is_file());
    assert_eq!(fs::read(&first).unwrap(), b"not a take");
    drop(r);
    let _ = fs::remove_dir_all(base);
}

/// The copy deleted, with nothing put back, between the start of Save
/// Again and its write: the save makes it again in place.
#[test]
fn a_copy_deleted_while_saving_is_made_again_in_place() {
    let (mut r, base) = save_fixture("replaced-copy-deleted");
    let takes = base.join("takes");
    let first = r.save_copy(&takes, "", None).unwrap();
    append_second(&mut r, 2);
    let pending = r.begin_save(&takes, "", None, &[]).unwrap();
    assert_eq!(pending.dest(), first);
    fs::remove_dir_all(&first).unwrap();
    let copy = pending.write().unwrap();
    assert_eq!(copy.dir, first);
    assert!(first.join("audio-001.wav").is_file());
    assert!(first.join("session.json").is_file());
    drop(r);
    let _ = fs::remove_dir_all(base);
}
