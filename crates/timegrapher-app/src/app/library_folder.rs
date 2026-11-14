//! The library folder: finding it, reading it, and moving it.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use timegrapher_core::recorder;

/// Saved takes whose hours are still WAVs (saved before hours were kept
/// as FLAC, #503, or stopped before one was converted), kept as FLAC one
/// at a time on a thread of their own.
pub(super) struct WavKeep {
    /// How many takes are done, of `of`.
    done: Arc<AtomicUsize>,
    of: usize,
    /// When it is over: the hours converted, and the takes that couldn't
    /// be, each with why.
    rx: mpsc::Receiver<(usize, Vec<String>)>,
    /// The take on show ([`TimegrapherApp::take_on_show`]), kept up to
    /// date each frame: its hours are left as WAVs for another time, so
    /// none is converted from under its analysis.
    open: Arc<Mutex<Option<PathBuf>>>,
}

impl WavKeep {
    /// Whether take folder `dir` is the take on show, holds it, or lies
    /// in it (a session folder's take of one position, Trevor).
    fn is_open(open: &Mutex<Option<PathBuf>>, dir: &Path) -> bool {
        open.lock().map_or(true, |o| {
            o.as_deref()
                .is_some_and(|o| o.starts_with(dir) || dir.starts_with(o))
        })
    }
}

impl WavKeep {
    /// The status bar's fact while it works.
    pub(super) fn words(&self) -> String {
        let done = self.done.load(Ordering::Relaxed).min(self.of);
        format!("Keeping saved takes as FLAC: {done} of {}", self.of)
    }
}

/// Why Move Watches… waits while a take is recorded into the library.
const RECORDING_INSIDE: &str =
    "The Watches folder moves once the take being recorded into it is stopped: press Stop, then \
     try again.";

/// Move Library… while saved takes are being kept as FLAC.
pub(super) const WAV_KEEP_WAIT: &str =
    "The library moves once its saved takes are kept as FLAC: try again when the status bar \
     no longer says so.";

/// The status fact's hover.
pub(super) const WAV_KEEP_HINT: &str = "Takes saved before recordings were kept as FLAC, \
     or stopped before an hour was converted, are converted an hour at a time. Each hour is \
     checked sample for sample before its WAV is deleted, and a take being recorded is left \
     alone.";

impl TimegrapherApp {
    /// The library's folder: the one chosen, else `~/Timegrapher Data`.
    pub(super) fn library_root(&self) -> Option<PathBuf> {
        self.library_dir.clone().or_else(library::default_location)
    }

    /// Read the library afresh: none yet where its folder holds no
    /// `library.toml`.
    pub(super) fn read_library(&mut self) {
        // Let go of this copy's own lock first, or the new one finds it
        // held and reads as another copy's.
        self.library = Ok(None);
        self.library = match self.library_root() {
            Some(root) if library::Library::is_library(&root) => {
                library::Library::open(&root).map(Some)
            }
            _ => Ok(None),
        };
    }

    /// At start-up: keep as FLAC every saved hour in the library still kept
    /// as a WAV, on a thread of its own. Not when another copy of the app
    /// has the library: that copy does it.
    pub(super) fn keep_saved_wavs(&mut self) {
        let Ok(Some(lib)) = &self.library else { return };
        if lib.read_only.is_some() {
            return;
        }
        let mut dirs: Vec<PathBuf> = lib
            .watches
            .iter()
            .chain([&lib.unsorted])
            .flat_map(|e| e.takes.iter().map(|t| e.take_dir(t)))
            .flat_map(|d| recorder::holding_wav(&d))
            .collect();
        dirs.dedup();
        if dirs.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let done = Arc::new(AtomicUsize::new(0));
        let count = done.clone();
        let open = Arc::new(Mutex::new(self.take_on_show()));
        let on_show = open.clone();
        self.wav_keep = Some(WavKeep {
            done,
            of: dirs.len(),
            rx,
            open,
        });
        std::thread::spawn(move || {
            let (mut hours, mut failed) = (0, Vec::new());
            for d in dirs {
                let stop = || WavKeep::is_open(&on_show, &d);
                match recorder::keep_saved_as_flac_unless(&d, &stop) {
                    Ok(n) => hours += n,
                    Err(e) => failed.push(e),
                }
                count.fetch_add(1, Ordering::Relaxed);
            }
            let _ = tx.send((hours, failed));
        });
    }

    /// Take in the end of [`Self::keep_saved_wavs`]: the library read again
    /// for its takes' sizes, and any take it couldn't convert logged and
    /// said.
    pub(super) fn poll_wav_keep(&mut self) {
        let Some(k) = &self.wav_keep else { return };
        if let Ok(mut o) = k.open.lock() {
            *o = self.take_on_show();
        }
        // A thread that died sends nothing: its end all the same, so the
        // status bar doesn't keep its count (Trevor).
        let (hours, failed) = match k.rx.try_recv() {
            Ok(end) => end,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => (
                0,
                vec!["the conversion stopped before it finished".to_string()],
            ),
        };
        self.wav_keep = None;
        let unit = if hours == 1 { "hour" } else { "hours" };
        applog::info(&format!("Kept {hours} saved {unit} as FLAC"));
        for e in &failed {
            applog::warn(&format!("Couldn't keep a saved hour as FLAC: {e}"));
        }
        if let Some(e) = failed.first() {
            let m = format!("Couldn't keep a saved hour as FLAC, so it stays a WAV: {e}");
            self.message = Some((m, true));
        }
        self.read_library();
    }

    /// Act on a button in Settings' Watches Folder section.
    pub(super) fn library_action(&mut self, act: settings_window::LibraryAction) {
        use settings_window::LibraryAction;
        match act {
            LibraryAction::Show => {
                if let Some(root) = self.library_root() {
                    if let Err(e) = reveal(&root) {
                        self.error(format!("Couldn't open {}: {e}", root.display()));
                    }
                }
            }
            LibraryAction::Move => {
                let (Some(root), Ok(Some(_))) = (self.library_root(), &self.library) else {
                    return;
                };
                // Its takes are being converted where they are now.
                if self.wav_keep.is_some() {
                    self.message = Some((WAV_KEEP_WAIT.into(), false));
                    return;
                }
                // A take still growing inside it would be copied half
                // written.
                if self.running()
                    && self
                        .session
                        .saved_to()
                        .is_some_and(|p| p.starts_with(&root))
                {
                    self.message = Some((RECORDING_INSIDE.into(), false));
                    return;
                }
                let mut d = rfd::FileDialog::new().set_title("Move the Library Into a Folder");
                if let Some(parent) = root.parent() {
                    d = d.set_directory(parent);
                }
                let name = root
                    .file_name()
                    .map_or_else(|| library::DEFAULT_NAME.into(), |n| n.to_owned());
                self.pick(Picker::one(d, Pick::Folder, move |a, parent| {
                    a.library_move_ask = Some(parent.join(name));
                }));
            }
            LibraryAction::Choose => {
                let d = rfd::FileDialog::new()
                    .set_title("Choose a Watches Folder, or an Empty Folder for a New One");
                self.pick(Picker::one(d, Pick::Folder, Self::choose_library));
            }
            LibraryAction::UseDefault => self.use_library(None),
        }
    }

    /// Use the folder picked in Choose…'s dialog as the library, unless it
    /// holds files but no watches.
    fn choose_library(&mut self, dir: PathBuf) {
        let empty = std::fs::read_dir(&dir).map_or(true, |mut d| d.next().is_none());
        if !library::Library::is_library(&dir) && !empty {
            return self.error(format!(
                "{} holds files but no watches: choose a Watches folder, or an empty folder for a \
                 new one",
                dir.display()
            ));
        }
        self.use_library(Some(dir));
    }

    /// Use the library at `dir` (`None`: the default) from now on.
    pub(super) fn use_library(&mut self, dir: Option<PathBuf>) {
        // The default chosen by name is the default.
        self.library_dir = dir.filter(|d| Some(d) != library::default_location().as_ref());
        self.read_library();
    }

    /// Read the Watches folder again when the window comes back to the
    /// front, so a take another program added while the app was open
    /// (`timegrapher watches add-take`, a sync) is listed without a
    /// restart. Its cache keeps this quick: only a take whose files
    /// changed is read again.
    pub(super) fn reread_library_on_focus(&mut self, ctx: &egui::Context) {
        let back = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(true)))
        });
        if let (true, Ok(Some(l))) = (back, &mut self.library) {
            l.scan();
        }
    }

    /// Move Library… asks first, saying what moves and where.
    pub(super) fn library_move_dialog(&mut self, ctx: &egui::Context) {
        let (Some(to), Some(from)) = (self.library_move_ask.clone(), self.library_root()) else {
            return;
        };
        let what = match &self.library {
            Ok(Some(l)) => format!(
                "Its {} and their takes ({}) are moved there,",
                if l.watches.len() == 1 {
                    "1 watch".to_string()
                } else {
                    format!("{} watches", l.watches.len())
                },
                size_text(l.bytes())
            ),
            _ => "Everything in it is moved there,".to_string(),
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("move library")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(
                RichText::new(format!("Move your watches to {}?", to.display()))
                    .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(format!(
                "{what} and the app keeps them there from now on. Onto another disk they are \
                 copied, checked and only then removed from {}.",
                from.display()
            ));
            ui.add_space(8.0);
            if let Some(i) = theme::dialog_buttons(
                ui,
                &[
                    DialogButton::new(Role::Default, "Move", ""),
                    DialogButton::new(Role::Cancel, "Cancel", ""),
                ],
            ) {
                choice = Some(i == 0);
            }
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(false);
        }
        if let Some(go) = choice {
            self.library_move_ask = None;
            if go {
                self.move_library(from, to);
            }
        }
    }

    /// Move the library on a thread of its own: a copy onto another disk
    /// can take a while.
    pub(super) fn move_library(&mut self, from: PathBuf, to: PathBuf) {
        // A library this copy of the app holds can't be moved.
        if let Ok(Some(l)) = &mut self.library {
            l.unlock();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let dest = to.clone();
        std::thread::spawn(move || {
            let _ = tx.send(library::move_library(&from, &dest));
        });
        self.library_moving = Some((to, rx));
    }

    /// The library moved from `from` to `to`: the take open from it,
    /// the one saved into it and its next Save Again follow, as they do
    /// a take moved on its own ([`Self::take_moved`]).
    pub(super) fn library_moved(&mut self, from: &Path, to: &Path) {
        let saved = self.session.saved_to().map(Path::to_path_buf);
        if let Some((take, rest)) = saved
            .as_ref()
            .and_then(|t| Some((t, t.strip_prefix(from).ok()?)))
        {
            // Its next save goes into the take where it is now.
            let now = to.join(rest);
            if let Some(r) = self.session.recorder.as_mut() {
                r.copy_moved(take, &now);
            }
        }
        // Every save noted, and the recording opened, by their folders.
        self.session.saved_moved(from, to);
        if let Ok(rest) = Path::new(&self.opened_path).strip_prefix(from) {
            self.opened_path = to.join(rest).display().to_string();
        }
        if let Ok(rest) = Path::new(self.file_path.trim()).strip_prefix(from) {
            self.file_path = to.join(rest).display().to_string();
        }
    }

    /// The move's outcome, once it is done.
    pub(super) fn poll_library_move(&mut self, ctx: &egui::Context) {
        let Some((to, rx)) = &self.library_moving else {
            return;
        };
        let to = to.clone();
        match rx.try_recv() {
            Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
            Ok(Ok(left)) => {
                // The whole library is at `to` either way, so it is used
                // there, and so is every take open from it (#1646).
                self.library_moving = None;
                if let Some(from) = self.library_root() {
                    self.library_moved(&from, &to);
                }
                self.use_library(Some(to.clone()));
                match left {
                    None => self.info_msg(format!("Moved your watches to {}.", to.display())),
                    Some(why) => self.error(why),
                }
            }
            Ok(Err(e)) => {
                self.library_moving = None;
                self.read_library();
                self.error(format!("Your watches couldn't be moved: {e}"));
            }
            Err(TryRecvError::Disconnected) => {
                self.library_moving = None;
                self.read_library();
                self.error("Moving your watches stopped without finishing.".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conversion thread that dies sends nothing: that is its end too,
    /// said, so the status bar doesn't keep its count (Trevor, #1120).
    #[test]
    fn a_conversion_that_dies_ends_and_says_so() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let (tx, rx) = mpsc::channel();
        drop(tx);
        app.wav_keep = Some(WavKeep {
            done: Arc::new(AtomicUsize::new(0)),
            of: 2,
            rx,
            open: Arc::default(),
        });
        app.poll_wav_keep();
        assert!(app.wav_keep.is_none());
        let (m, bad) = app.message.clone().expect("said");
        assert!(bad && m.contains("stopped before it finished"), "{m}");
    }

    /// While it works, the window draws again by itself, so the count
    /// moves and the fact goes when it ends without the mouse being moved
    /// (Walter's walk of #1120).
    #[test]
    fn the_window_draws_again_while_it_works() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let (_tx, rx) = mpsc::channel();
        app.wav_keep = Some(WavKeep {
            done: Arc::new(AtomicUsize::new(0)),
            of: 2,
            rx,
            open: Arc::default(),
        });
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let delay = |app: &mut TimegrapherApp| {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.status_facts(ui));
            });
            out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
        };
        delay(&mut app);
        assert!(delay(&mut app) <= std::time::Duration::from_secs(1));
    }
}
