//! Keeping a take as its analysis alone: from its sound read again once
//! it is recorded ([`AnalysisSave`]), or as it records, its sound read as
//! it comes and never written (#1514): a Power Reserve Test with Analysis
//! Only picked. Its analysis is kept on the disk as it goes
//! ([`timegrapher_core::checkpoint`]), and written as the take when the
//! test ends.

use super::*;
use timegrapher_core::analysis_take::{self, Stored};
use timegrapher_core::keeper::{Checkpoints, KeepAs, Keeper};
use timegrapher_core::recorder::NamedFiles;

/// The session being kept as its analysis alone
/// ([`timegrapher_core::analysis_take`]): its sound read again in the
/// background, then the new take written.
pub(super) struct AnalysisSave {
    pub(super) rx: std::sync::mpsc::Receiver<Result<PathBuf, String>>,
    /// Seconds of sound read so far, over both readings of it.
    pub(super) progress: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub(super) cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The take's length, seconds.
    pub(super) total_s: f64,
}

impl Drop for AnalysisSave {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // Cancelled just as it finished: the folder it wrote, never
        // reported, goes, so Cancel leaves nothing written.
        if let Ok(Ok(dir)) = self.rx.try_recv() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// A take kept as its analysis alone as it records.
#[derive(Default)]
pub(super) struct Kept {
    /// The reading the sound goes to, while the take records.
    run: Option<Keeper>,
    /// The last failure to keep the analysis on the disk that was said,
    /// so each is said once.
    said: Option<String>,
    /// The take being written, once the recording has ended, and the
    /// recording's folder.
    writing: Option<(std::sync::mpsc::Receiver<Written>, PathBuf)>,
    /// The take whose write failed, kept here and written again a minute
    /// after.
    unwritten: Option<(Unwritten, Instant)>,
}

/// What a take kept as its analysis alone is written from, kept to write
/// it again when a write fails.
struct Unwritten {
    /// The folder it was recorded in, holding its analysis kept as it
    /// went: never the session's once a new one has begun.
    recording: PathBuf,
    stored: Box<Stored>,
    companions: NamedFiles,
    dest: PathBuf,
    total_s: f64,
    metadata: crate::session::MetadataSnapshot,
}

/// How a write went, with what it was written from.
type Written = (Result<PathBuf, String>, Option<Unwritten>);

impl TimegrapherApp {
    /// Whether the take records into a reading of its own, its sound never
    /// written.
    pub(super) fn kept_running(&self) -> bool {
        self.kept_live.run.is_some()
    }

    /// Record the session just started as its analysis alone: none of its
    /// sound written, all of it read as it comes, and the analysis kept on
    /// the disk every 10 minutes in the recording's folder. False, with
    /// why said, when it can't be: the sound is kept then.
    pub(super) fn start_kept(&mut self) -> bool {
        let manifest = self.take_manifest();
        // Read as the take will be opened: its beat rate and lift, and the
        // escape teeth its session file and calibre give (#1324); its beat
        // shape on the minute after settling.
        let cfg = self.entry_settings(&manifest).stream;
        let shapes = analysis_take::shape_stretches(&manifest, f64::INFINITY);
        let Some(r) = self.session.recorder.as_mut() else {
            return false;
        };
        if let Err(e) = r.without_sound() {
            self.error(format!(
                "The test can't keep its analysis alone ({e}): it keeps its sound."
            ));
            return false;
        }
        self.kept_live = Kept {
            run: Some(Keeper::start(KeepAs {
                sample_rate: r.info().sample_rate,
                bytes_per_frame: r.bytes_per_frame(),
                cfg,
                shapes,
                software: report::software(),
                checkpoints: Some(Checkpoints {
                    take: r.dir().to_path_buf(),
                    every_s: checkpoint_every_s(),
                }),
            })),
            ..Kept::default()
        };
        true
    }

    /// Follow the take kept as it records: say once when its analysis
    /// can't be kept on the disk, take in its write once done, and write
    /// it again a minute after a write failed.
    pub(super) fn kept_follow(&mut self) {
        if let Some(k) = &self.kept_live.run {
            let failed = k.checkpointed().failed;
            if failed != self.kept_live.said {
                if let Some(e) = &failed {
                    let m = format!(
                        "The test's analysis couldn't be kept on the disk as it goes ({e}). \
                         The test carries on, and its take is still kept when it ends."
                    );
                    applog::error(&m);
                    self.message = Some((m, true));
                }
                self.kept_live.said = failed;
            }
        }
        self.kept_written();
        let again = self
            .kept_live
            .unwritten
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= STREAM_RETRY);
        if again {
            if let Some((u, _)) = self.kept_live.unwritten.take() {
                let at = u.recording.clone();
                self.write_kept(at, Box::new(move || Ok(u)));
            }
        }
    }

    /// How long until a failed write is tried again, for the window to
    /// draw then on its own.
    pub(super) fn kept_retry_in(&self) -> Option<std::time::Duration> {
        let (_, at) = self.kept_live.unwritten.as_ref()?;
        Some(STREAM_RETRY.saturating_sub(at.elapsed()))
    }

    /// The ended test's take, kept as its analysis alone in a new folder
    /// `dest`: its analysis finished, and the take written beside the
    /// recording's other files on a thread of its own.
    pub(super) fn save_kept(&mut self, dest: PathBuf) {
        let Some(keeper) = self.kept_live.run.take() else {
            return;
        };
        let prepared = self.kept_companions();
        let metadata = self.session.metadata_snapshot(self.save_metadata());
        let total_s = self
            .session
            .recorder
            .as_ref()
            .map_or(0.0, |r| r.duration_s());
        let Some(recording) = self
            .session
            .recorder
            .as_ref()
            .map(|r| r.dir().to_path_buf())
        else {
            return;
        };
        let companions = match prepared {
            Ok(c) => c,
            Err(e) => {
                // No take without its session file and results: the
                // analysis kept as it went stays in the recording's folder.
                drop(keeper);
                return self.error(format!(
                    "The test's take couldn't be kept ({e}). Its analysis, up to its last save \
                     at most 10 minutes before the end, is kept in {}.",
                    recording.display()
                ));
            }
        };
        if let Some(t) = self.reserve.as_mut() {
            t.saving = true;
        }
        let at = recording.clone();
        self.write_kept(
            at,
            Box::new(move || {
                let stored = keeper.finish().map_err(|e| e.to_string())?;
                Ok(Unwritten {
                    recording,
                    stored: Box::new(stored),
                    companions,
                    dest,
                    total_s,
                    metadata,
                })
            }),
        );
    }

    /// Every file of the recording but its sound, as the take keeps them:
    /// its session file saying it has no sound, and the test's results.
    fn kept_companions(&mut self) -> Result<NamedFiles, String> {
        let mut manifest = self.take_manifest();
        manifest.audio = Some(false);
        let result = self
            .reading()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| format!("Couldn't prepare the saved reading: {e}"))?;
        let text = toml::to_string_pretty(&manifest)
            .map_err(|e| format!("Couldn't prepare {MANIFEST}: {e}"))?;
        let kept = self.results_files()?;
        let mut required = vec![(MANIFEST, text.as_bytes())];
        required.extend(kept.iter().map(|(n, b)| (*n, b.as_slice())));
        let calibre = self.calibre.clone();
        let r = self
            .session
            .recorder
            .as_mut()
            .ok_or("the recording is gone")?;
        let (_, files) = r
            .analysis_copy_files(&calibre, result.as_ref(), &required)
            .map_err(|e| e.to_string())?;
        Ok(files)
    }

    /// Write the take `from` gives, on a thread of its own.
    fn write_kept(
        &mut self,
        recording: PathBuf,
        from: Box<dyn FnOnce() -> Result<Unwritten, String> + Send>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let written = match from() {
                Ok(u) => match analysis_take::save(&u.dest, &u.companions, &u.stored) {
                    Ok(dir) => (Ok(dir), Some(u)),
                    Err(e) => (Err(e.to_string()), Some(u)),
                },
                Err(e) => (Err(e), None),
            };
            let _ = tx.send(written);
        });
        self.kept_live.writing = Some((rx, recording));
    }

    /// The take written, once it is: the session saved there, and the
    /// analysis kept on the disk as it went let go; or, when it couldn't
    /// be, kept to write again.
    fn kept_written(&mut self) {
        let Some((rx, recording)) = &self.kept_live.writing else {
            return;
        };
        let recording = recording.clone();
        let written = match rx.try_recv() {
            Ok(w) => w,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => (Err("it stopped part way".into()), None),
        };
        self.kept_live.writing = None;
        match written {
            (Ok(dir), Some(u)) => {
                self.info_msg(format!(
                    "Kept {} of analysis in {} ({}), without the sound.",
                    length_text(u.total_s),
                    dir.display(),
                    size_text(folder_size(&dir))
                ));
                self.kept_in_place(u, dir);
                if let Ok(Some(l)) = &mut self.library {
                    l.scan();
                }
            }
            (Err(e), Some(u)) => {
                self.error(format!(
                    "The test's take couldn't be written in {} ({e}). Its analysis is held in \
                     the app and written again in a minute: keep the app open until it's kept.",
                    u.dest.display()
                ));
                self.kept_live.unwritten = Some((u, Instant::now()));
            }
            (r, None) => {
                let e = r.err().unwrap_or_default();
                self.error(format!(
                    "The test's analysis couldn't be finished ({e}). What was kept as it \
                     went is in {}.",
                    recording.display()
                ));
                if self.is_recording(&recording) {
                    if let Some(t) = self.reserve.as_mut() {
                        t.saving = false;
                    }
                }
            }
        }
    }
}

impl TimegrapherApp {
    /// Whether `dir` is the folder the session records into now.
    fn is_recording(&self, dir: &Path) -> bool {
        self.session
            .recorder
            .as_ref()
            .is_some_and(|r| r.dir() == dir)
    }

    /// The take written from `u` in `dir`: the session saved there while it
    /// is still the one recorded in `u`'s folder, and that folder's
    /// analysis kept as it went let go; the whole folder, once a new
    /// session has left it behind for its parts.
    fn kept_in_place(&mut self, u: Unwritten, dir: PathBuf) {
        if !self.is_recording(&u.recording) {
            // Already gone when it held no parts yet.
            let gone = std::fs::remove_dir_all(&u.recording);
            if let (Err(e), true) = (gone, u.recording.exists()) {
                applog::warn(&format!("the recording kept for its analysis stays: {e}"));
            }
            return;
        }
        self.session.mark_saved(u.total_s, dir.clone(), u.metadata);
        if let Err(e) = timegrapher_core::checkpoint::remove(&u.recording) {
            applog::warn(&format!("the analysis kept as it went stays: {e}"));
        }
        if let Some(t) = self.reserve.as_mut() {
            t.saving = false;
            t.saved = Some(dir);
        }
    }

    /// A failed write tried again at the next follow, as a minute on.
    #[cfg(test)]
    pub(super) fn kept_write_now(&mut self) {
        if let Some((_, at)) = self.kept_live.unwritten.as_mut() {
            *at -= STREAM_RETRY;
        }
    }

    /// Whether a take kept as its analysis alone is being written, for
    /// the window to draw until it is and show it saved.
    pub(super) fn kept_writing(&self) -> bool {
        self.kept_live.writing.is_some()
    }

    /// Whether a take kept as its analysis alone is being written: what
    /// would begin a new session waits until it is, and says so.
    pub(super) fn kept_writing_busy(&mut self) -> bool {
        if self.kept_live.writing.is_none() {
            return false;
        }
        self.info_msg("Writing the test's take: wait until it is kept.".into());
        true
    }
}

#[cfg(test)]
thread_local! {
    /// How often a test's analysis is kept on the disk, in place of every
    /// 10 minutes, so a short run is kept.
    pub(super) static EVERY_S: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

/// How often the analysis is kept on the disk as it goes.
fn checkpoint_every_s() -> f64 {
    #[cfg(test)]
    if let Some(s) = EVERY_S.with(std::cell::Cell::get) {
        return s;
    }
    timegrapher_core::checkpoint::EVERY_S
}

/// Keep block `b` in `session`'s recording: its sound written, and read
/// as it comes by `kept`'s reading when the take is kept as its analysis
/// alone.
pub(super) fn keep_block(
    session: &mut crate::session::Session,
    kept: &Kept,
    b: &timegrapher_core::capture::Block,
) -> Result<(), CaptureFailure> {
    // An error stops the queued sound being processed, and no later
    // input error replaces it.
    if let Some(r) = session.recorder.as_mut() {
        r.write(b)
            .map_err(|e| CaptureFailure::Recording(e.to_string()))?;
    }
    match &kept.run {
        Some(k) if !k.push(&b.samples) => Err(CaptureFailure::Recording(
            "the analysis of the sound stopped".into(),
        )),
        _ => Ok(()),
    }
}
