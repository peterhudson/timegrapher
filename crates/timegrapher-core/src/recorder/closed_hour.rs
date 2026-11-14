//! Opening a closed hour of a recording for a save, as its WAV or its
//! FLAC, without a look before the open that the FLAC thread can race;
//! and opening a saved copy's hour to grow it in place, never through a
//! link (#1640).

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// The closed hour `path`, held open, with its length: the WAV, or its
/// FLAC once it has been kept as one. The WAV is tried first and, when
/// it can't be opened, its FLAC, never judged by a look before the open: the thread keeping it as FLAC deletes the WAV once its
/// FLAC is in place, and between a look and an open that failed the save
/// (#1486). A WAV that is a link, or not a file, is refused as before,
/// FLAC or not.
pub(super) fn open_closed_hour(path: &Path) -> io::Result<(PathBuf, File, u64)> {
    let open = |p: &Path| -> io::Result<(File, u64)> {
        super::regular_file(p)?;
        let f = File::open(p)?;
        let len = f.metadata()?.len();
        Ok((f, len))
    };
    match open(path) {
        Ok((f, len)) => Ok((path.to_path_buf(), f, len)),
        Err(e) => {
            let flac = crate::flac::flac_path(path);
            let refused = e.kind() == io::ErrorKind::InvalidInput;
            if refused || !crate::flac::is_wav(path) {
                return Err(e);
            }
            let (f, len) = open(&flac)?;
            Ok((flac, f, len))
        }
    }
}

/// The saved copy's hour `to`, opened to be written in place: refused
/// (InvalidInput) unless it is a regular file of its own, never a link,
/// whose write would land on the file it points at (#1640). On unix the
/// file opened must be the very one looked at, with no other name, since
/// a hard link would carry the write to that name too.
pub(super) fn open_in_place(to: &Path) -> io::Result<File> {
    let seen = super::regular_file(to)?;
    let out = OpenOptions::new().write(true).open(to)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let got = out.metadata()?;
        if (got.dev(), got.ino()) != (seen.dev(), seen.ino()) || got.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a file of its own", to.display()),
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = seen;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A closed hour is opened as its WAV, else its FLAC, and still is when
    /// the thread keeping it as FLAC deletes the WAV during the open
    /// (#1486: a save begun at the Stop failed on Windows).
    #[test]
    fn a_closed_hour_opens_though_its_wav_is_deleted_meanwhile() {
        let dir = std::env::temp_dir().join(format!("tg-closed-hour-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (wav, flac) = (dir.join("audio-001.wav"), dir.join("audio-001.flac"));
        let opened = |p: &Path| open_closed_hour(p).map(|(at, _, len)| (at, len));
        fs::write(&wav, b"wav sound").unwrap();
        assert_eq!(opened(&wav).unwrap(), (wav.clone(), 9));
        fs::write(&flac, b"flac").unwrap();
        assert_eq!(opened(&wav).unwrap(), (wav.clone(), 9));
        fs::remove_file(&wav).unwrap();
        assert_eq!(opened(&wav).unwrap(), (flac.clone(), 4));
        fs::remove_file(&flac).unwrap();
        assert_eq!(opened(&wav).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(opened(&flac).unwrap_err().kind(), io::ErrorKind::NotFound);
        // A WAV that is a link is refused, with its FLAC beside it or not.
        #[cfg(unix)]
        {
            let elsewhere = dir.join("elsewhere.wav");
            fs::write(&elsewhere, b"wav sound").unwrap();
            std::os::unix::fs::symlink(&elsewhere, &wav).unwrap();
            assert_eq!(
                opened(&wav).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            fs::write(&flac, b"flac").unwrap();
            assert_eq!(
                opened(&wav).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            fs::remove_file(&wav).unwrap();
            fs::remove_file(&flac).unwrap();
        }
        // The WAV deleted while it is opened, as the FLAC thread does once
        // the FLAC is in place: every open finds one or the other.
        for _ in 0..2000 {
            fs::write(&wav, b"wav sound").unwrap();
            fs::write(&flac, b"flac").unwrap();
            let gone = std::thread::spawn({
                let wav = wav.clone();
                move || fs::remove_file(wav)
            });
            let got = opened(&wav);
            gone.join().unwrap().unwrap();
            let (at, len) = got.unwrap();
            assert_eq!(len, if at == wav { 9 } else { 4 });
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Only a regular file of its own is opened to grow in place: a link,
    /// to a file or a folder, and on unix a second name for a file, are
    /// refused, and the file they lead to is left as it was (#1640).
    #[test]
    fn only_a_file_of_its_own_is_opened_to_grow() {
        let dir = std::env::temp_dir().join(format!("tg-in-place-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let own = dir.join("audio-001.wav");
        fs::write(&own, b"sound").unwrap();
        assert!(open_in_place(&own).is_ok());
        assert_eq!(
            open_in_place(&dir.join("gone.wav")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            open_in_place(&dir).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        #[cfg(unix)]
        {
            let other = dir.join("other.wav");
            fs::write(&other, b"another take").unwrap();
            let link = dir.join("link.wav");
            std::os::unix::fs::symlink(&other, &link).unwrap();
            assert_eq!(
                open_in_place(&link).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            let hard = dir.join("hard.wav");
            fs::hard_link(&other, &hard).unwrap();
            assert_eq!(
                open_in_place(&hard).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(fs::read(&other).unwrap(), b"another take");
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
