//! A take's folder in a watch, made as it is named (#1988, #1862).
//!
//! A take recorded into a watch is named by the minute it started. Its
//! name used to be picked as the first free one, and its folder made only
//! at its first save: two takes started in the same minute (two benches,
//! or a take whose first save failed and another started) could be given
//! one name, and the second save would write into the first take. Now
//! the folder is made as the name is given, in one step, so a name once
//! given is never given again.

use super::Recorder;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

impl Recorder {
    /// Give the first saved copy a take folder of its own in `parent` (a
    /// watch's takes), named `base`, or `base 2` and on where that is
    /// taken, rather than by its start, watch and calibre. The folder is
    /// made now ([`crate::library::make_unused_dir`]), so no other take
    /// is given it before the first save writes there. A copy already
    /// given its folder in `parent` (a first save that failed, tried
    /// again, or the copy saved there before) keeps it and makes no
    /// other. Returns the copy's folder.
    pub fn claim_first_copy(&mut self, parent: &Path, base: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(parent)?;
        let canonical = fs::canonicalize(parent)?;
        let name = match self.kept_in(&canonical) {
            Some(name) => name,
            None => self.claim(parent, &canonical, base)?,
        };
        Ok(parent.join(name))
    }

    /// Where a save into `parent` writes: the copy saved there before
    /// (Save Again), or the folder made for it there and not yet written;
    /// else a new folder, named as the copy saved elsewhere (less any
    /// ` 2` it was given there) or by its start, watch and `calibre`,
    /// with ` 2` and on where that is taken.
    /// A save into another folder never writes into a take already there
    /// (#2401). Refused where the copy would be, or hold, the live
    /// recording. Returns its folder as the caller spelled `parent`, and
    /// whether it is a new one.
    pub(super) fn copy_dest(
        &self,
        parent: &Path,
        calibre: &str,
    ) -> io::Result<(PathBuf, SaveInto)> {
        // Resolve identities only for these checks. Keep the caller's
        // spelling for filesystem work and paths returned or shown to them.
        let canonical = fs::canonicalize(parent)?;
        let source = fs::canonicalize(&self.dir)?;
        let separate = |name: &OsStr| -> io::Result<PathBuf> {
            let dest = parent.join(name);
            let resolved = if dest.exists() {
                fs::canonicalize(&dest)?
            } else {
                canonical.join(name)
            };
            if resolved.starts_with(&source) || source.starts_with(&resolved) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the saved copy must be separate from the live recording folder",
                ));
            }
            Ok(dest)
        };
        if let Some(name) = self.kept_in(&canonical) {
            let dest = separate(&name)?;
            // Save Again writes there only while it is still this take's
            // copy: a take put in its place gets a new copy beside it
            // (#2437). A copy that is gone is made again as a new one, so
            // a folder put there meanwhile is never written into (#2401).
            if dest.exists() && still_this_take(&dest, &self.started_utc) {
                return Ok((dest, SaveInto::Again(self.started_utc.clone())));
            }
        }
        // Named as the copy saved elsewhere, without the number that copy
        // was given where its name was taken: a new copy where the name
        // is free reads by its start alone (#2439).
        let base = self
            .saved_name
            .as_deref()
            .map(unnumbered)
            .unwrap_or_else(|| self.copy_name(calibre));
        separate(&base)?;
        let free = crate::library::unused_dir(parent, &base.to_string_lossy());
        let name = free.file_name().map_or(base, ToOwned::to_owned);
        Ok((separate(&name)?, SaveInto::New))
    }

    /// Note `dir` as a copy saved: the one Save Again saves into, and the
    /// one a save into its folder updates.
    pub(super) fn note_copy(&mut self, dir: &Path) {
        let Some(name) = dir.file_name().map(ToOwned::to_owned) else {
            return;
        };
        self.saved_name = Some(name.clone());
        self.claimed = None;
        if let Some(parent) = dir.parent().and_then(|p| fs::canonicalize(p).ok()) {
            self.copies.retain(|(p, _)| *p != parent);
            self.copies.push((parent, name));
        }
    }

    /// The copy saved at `from` is now at `to` (filed under a watch).
    pub(super) fn copy_renamed(&mut self, from: &Path, to: &Path) {
        let parent = from.parent().and_then(|p| fs::canonicalize(p).ok());
        let name = from.file_name();
        self.copies
            .retain(|(p, n)| Some(p) != parent.as_ref() || Some(n.as_os_str()) != name);
        self.note_copy(to);
    }

    /// The name of this copy's folder in the folder resolved as
    /// `canonical`: the copy saved there, or one made there for it.
    fn kept_in(&self, canonical: &Path) -> Option<OsString> {
        if let Some((_, name)) = self.copies.iter().find(|(p, _)| p == canonical) {
            return Some(name.clone());
        }
        self.claimed
            .as_ref()
            .filter(|c| c.parent() == Some(canonical) && c.is_dir())
            .and_then(|c| c.file_name())
            .map(ToOwned::to_owned)
    }

    /// Make a new folder for this copy in `parent`, named `base` or with
    /// a number, and note it. Returns its name.
    fn claim(&mut self, parent: &Path, canonical: &Path, base: &str) -> io::Result<OsString> {
        let dir = crate::library::make_unused_dir(parent, base)?;
        let name = dir
            .file_name()
            .map(ToOwned::to_owned)
            .ok_or_else(|| io::Error::other("a take folder with no name"))?;
        self.claimed = Some(canonical.join(&name));
        Ok(name)
    }
}

/// Where a save writes, for [`super::PendingSave::write`] to check again
/// as it writes: the copy may be deleted and another take put in its
/// place after the save began (#2437).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SaveInto {
    /// A new copy: its folder must not be there yet (#2401).
    New,
    /// The copy saved before, of the take that started at this time: it
    /// is written into only while it still holds that take.
    Again(String),
}

impl SaveInto {
    /// Refuse to write into `dir` once it holds another take.
    pub(super) fn check(&self, dir: &Path) -> io::Result<()> {
        match self {
            SaveInto::Again(started) if !still_this_take(dir, started) => {
                Err(io::Error::other(format!(
                    "{} has changed since this take was saved there, so nothing \
                     was saved into it; the next save makes a new copy beside it",
                    dir.display()
                )))
            }
            _ => Ok(()),
        }
    }

    /// This, once [`SaveInto::check`] passes.
    pub(super) fn checked(&self, dir: &Path) -> io::Result<Self> {
        self.check(dir).map(|()| self.clone())
    }
}

/// Whether `dir`, a folder a recording that started at `started` was
/// saved into or given, still holds that take or nothing: gone (the save
/// makes it again), empty, or with a `session.json` that names that
/// start. Anything else there (another take copied in after the copy was
/// deleted, or a `session.json` that can't be read) is not written into
/// (#2437).
fn still_this_take(dir: &Path, started: &str) -> bool {
    match fs::read_dir(dir).map(|mut e| e.next().is_none()) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return true,
        Err(_) => return false,
        Ok(true) => return true,
        Ok(false) => {}
    }
    fs::read(dir.join(crate::take::RECORD))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .is_some_and(|v| v["started_utc"].as_str() == Some(started))
}

/// `name` less the ` 2`, ` 3`… a take's folder is given where its name
/// is taken ([`crate::library::make_unused_dir`]). A take's own name never
/// ends so: it is its start minute (`2026-11-26 05.30`, with
/// ` Power Reserve` for that test) or [`Recorder::copy_name`], which
/// holds no space.
fn unnumbered(name: &OsStr) -> OsString {
    let Some(text) = name.to_str() else {
        return name.to_owned();
    };
    match text.rsplit_once(' ') {
        Some((base, n))
            if !base.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && n.parse::<u64>().is_ok_and(|n| n >= 2)
                && !n.starts_with('0') =>
        {
            base.into()
        }
        _ => name.to_owned(),
    }
}

#[cfg(test)]
#[path = "replaced_copy_tests.rs"]
mod replaced_copy_tests;

#[cfg(test)]
mod tests {
    use super::super::tests::save_fixture;
    use crate::capture::Block;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, UNIX_EPOCH};

    fn files(dir: &Path) -> BTreeMap<std::ffi::OsString, Vec<u8>> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().unwrap().is_file())
            .map(|e| (e.file_name(), fs::read(e.path()).unwrap()))
            .collect()
    }

    /// Save As into another watch's takes, where a take of the same minute
    /// name is already saved, makes a folder of its own there and leaves
    /// that take's sound as it was; into the take's own watch it saves
    /// again in place, and into an empty folder it makes one (#2401).
    #[test]
    fn save_as_never_writes_into_another_takes_folder() {
        let (mut a, base) = save_fixture("save-as-a");
        let (mut b, other) = save_fixture("save-as-b");
        for (r, level) in [(&mut a, 0.25), (&mut b, -0.25)] {
            r.write(&Block {
                samples: vec![level; 8000],
                at: UNIX_EPOCH + Duration::from_secs(2),
            })
            .unwrap();
        }
        let name = "2026-11-26 05.30";
        let (a_takes, b_takes) = (base.join("A").join("takes"), base.join("B").join("takes"));
        a.claim_first_copy(&a_takes, name).unwrap();
        b.claim_first_copy(&b_takes, name).unwrap();
        let a_dir = a.save_copy(&a_takes, "", None).unwrap();
        let b_dir = b.save_copy(&b_takes, "", None).unwrap();
        assert_eq!(b_dir, b_takes.join(name));
        let b_before = files(&b_dir);
        let a_sound = fs::read(a_dir.join("audio-001.wav")).unwrap();
        assert_ne!(a_sound, b_before[std::ffi::OsStr::new("audio-001.wav")]);
        // Again into its own watch: the same folder.
        assert_eq!(a.save_copy(&a_takes, "", None).unwrap(), a_dir);
        // Into an empty folder: a take of its own there.
        let c_takes = base.join("C").join("takes");
        assert_eq!(a.save_copy(&c_takes, "", None).unwrap(), c_takes.join(name));
        // Into B's watch, already holding B's take of that name.
        let into_b = a.save_copy(&b_takes, "", None).unwrap();
        assert_eq!(into_b, b_takes.join(format!("{name} 2")));
        assert_eq!(fs::read(into_b.join("audio-001.wav")).unwrap(), a_sound);
        // Back into A's watch: A's own copy again, no other made there.
        assert_eq!(a.save_copy(&a_takes, "", None).unwrap(), a_dir);
        assert_eq!(fs::read_dir(&a_takes).unwrap().count(), 1);
        assert_eq!(a.save_copy(&b_takes, "", None).unwrap(), into_b);
        // Saved again there, it goes on into its own folder.
        assert_eq!(a.save_copy(&b_takes, "", None).unwrap(), into_b);
        // Into another empty folder after that: by its start minute
        // alone, not as the copy saved last (#2439).
        let e_takes = base.join("E").join("takes");
        assert_eq!(a.save_copy(&e_takes, "", None).unwrap(), e_takes.join(name));
        assert_eq!(a.claim_first_copy(&b_takes, name).unwrap(), into_b);
        // Named for yet another watch, it is given a folder there.
        assert_eq!(
            a.claim_first_copy(&base.join("D"), name).unwrap(),
            base.join("D").join(name)
        );
        assert_eq!(files(&b_dir), b_before);
        assert_eq!(fs::read(a_dir.join("audio-001.wav")).unwrap(), a_sound);
        drop((a, b));
        let _ = fs::remove_dir_all(base);
        let _ = fs::remove_dir_all(other);
    }

    /// A take given a number where its start minute was taken (two
    /// benches) reads by its start minute alone in a folder where that is
    /// free, and is numbered again only where it is not (#2439).
    #[test]
    fn a_numbered_take_saved_elsewhere_drops_its_number_where_free() {
        let (mut a, base) = save_fixture("save-as-number-a");
        let (mut b, other) = save_fixture("save-as-number-b");
        let name = "2026-11-26 05.30";
        let takes = base.join("A").join("takes");
        b.claim_first_copy(&takes, name).unwrap();
        a.claim_first_copy(&takes, name).unwrap();
        assert_eq!(
            a.save_copy(&takes, "", None).unwrap(),
            takes.join(format!("{name} 2"))
        );
        let free = base.join("B").join("takes");
        assert_eq!(a.save_copy(&free, "", None).unwrap(), free.join(name));
        let taken = base.join("C").join("takes");
        fs::create_dir_all(taken.join(name)).unwrap();
        assert_eq!(
            a.save_copy(&taken, "", None).unwrap(),
            taken.join(format!("{name} 2"))
        );
        drop((a, b));
        let _ = fs::remove_dir_all(base);
        let _ = fs::remove_dir_all(other);
    }

    /// Only the number a taken name is given comes off; a name that ends
    /// otherwise is kept whole (#2439).
    #[test]
    fn only_the_number_given_for_a_taken_name_comes_off() {
        let off = |s: &str| super::unnumbered(std::ffi::OsStr::new(s));
        assert_eq!(off("2026-11-26 05.30 2"), "2026-11-26 05.30");
        assert_eq!(off("2026-11-26 05.30 12"), "2026-11-26 05.30");
        assert_eq!(
            off("2026-11-26 05.30 Power Reserve 3"),
            "2026-11-26 05.30 Power Reserve"
        );
        for kept in [
            "2026-11-26 05.30",
            "2026-11-26 05.30 Power Reserve",
            "2026-11-26T05-30-00_Save_test",
            "2026-11-26 05.30 1",
            "2026-11-26 05.30 02",
            "2026-11-26 05.30 +2",
            " 2",
        ] {
            assert_eq!(off(kept), kept);
        }
    }

    /// A new copy's folder made by another save between choosing its name
    /// and writing it is never written into: the save is refused and the
    /// other save's file left as it is (#2401).
    #[test]
    fn a_new_copys_folder_made_meanwhile_is_left_alone() {
        let (mut r, base) = save_fixture("save-as-race");
        let takes = base.join("B").join("takes");
        let name = r.copy_name("");
        fs::create_dir_all(takes.join(&name)).unwrap();
        let pending = r.begin_save(&takes, "", None, &[]).unwrap();
        let dest = pending.dest().to_path_buf();
        assert_eq!(
            dest.file_name().unwrap().to_string_lossy(),
            format!("{} 2", name.to_string_lossy())
        );
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("audio-001.wav"), b"other").unwrap();
        assert!(pending.write().is_err());
        assert_eq!(fs::read(dest.join("audio-001.wav")).unwrap(), b"other");
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 1);
        drop(r);
        let _ = fs::remove_dir_all(base);
    }

    /// Two takes started in the same minute, each named before either is
    /// saved (two benches), get a folder each: the second save never
    /// writes into the first take.
    #[test]
    fn two_takes_named_in_one_minute_get_a_folder_each() {
        let (mut a, base) = save_fixture("claim-a");
        let (mut b, other) = save_fixture("claim-b");
        let takes = base.join("Pepsi").join("takes");
        let first = a.claim_first_copy(&takes, "2026-11-25 06.52").unwrap();
        let second = b.claim_first_copy(&takes, "2026-11-25 06.52").unwrap();
        assert_eq!(first, takes.join("2026-11-25 06.52"));
        assert_eq!(second, takes.join("2026-11-25 06.52 2"));
        assert!(first.is_dir() && second.is_dir());
        let who = |r: &mut super::Recorder, w: &'static [u8]| {
            r.save_copy_with_files(&takes, "", None, &[("who.txt", w)])
                .unwrap()
        };
        assert_eq!(who(&mut a, b"a"), first);
        assert_eq!(who(&mut b, b"b"), second);
        assert_eq!(fs::read(first.join("who.txt")).unwrap(), b"a");
        assert_eq!(fs::read(second.join("who.txt")).unwrap(), b"b");
        drop((a, b));
        let _ = fs::remove_dir_all(base);
        let _ = fs::remove_dir_all(other);
    }

    /// Asked again before its first save is written (a save that failed,
    /// tried again), a take keeps the folder it was given and makes no
    /// other; given a folder in another watch's takes, it is given one
    /// there.
    #[test]
    fn a_take_named_again_keeps_its_folder() {
        let (mut r, base) = save_fixture("claim-again");
        let takes = base.join("Pepsi").join("takes");
        let dir = r.claim_first_copy(&takes, "2026-11-25 06.52").unwrap();
        assert_eq!(r.claim_first_copy(&takes, "2026-11-25 06.52").unwrap(), dir);
        assert_eq!(fs::read_dir(&takes).unwrap().count(), 1);
        let elsewhere = base.join("Explorer").join("takes");
        let moved = r.claim_first_copy(&elsewhere, "2026-11-25 06.52").unwrap();
        assert_eq!(moved, elsewhere.join("2026-11-25 06.52"));
        assert!(moved.is_dir());
        drop(r);
        let _ = fs::remove_dir_all(base);
    }
}
