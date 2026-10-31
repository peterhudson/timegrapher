//! The library folder: finding it, reading it, and moving it.

use super::*;

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
                let mut d = rfd::FileDialog::new().set_title("Move the Library Into a Folder");
                if let Some(parent) = root.parent() {
                    d = d.set_directory(parent);
                }
                let Some(parent) = d.pick_folder() else {
                    return;
                };
                let name = root
                    .file_name()
                    .map_or_else(|| library::DEFAULT_NAME.into(), |n| n.to_owned());
                self.library_move_ask = Some(parent.join(name));
            }
            LibraryAction::Choose => {
                let Some(dir) = rfd::FileDialog::new()
                    .set_title("Choose a Watches Folder, or an Empty Folder for a New One")
                    .pick_folder()
                else {
                    return;
                };
                let empty = std::fs::read_dir(&dir).map_or(true, |mut d| d.next().is_none());
                if !library::Library::is_library(&dir) && !empty {
                    return self.error(format!(
                        "{} holds files but no watches: choose a Watches folder, or an empty folder \
                         for a new one",
                        dir.display()
                    ));
                }
                self.use_library(Some(dir));
            }
            LibraryAction::UseDefault => self.use_library(None),
        }
    }

    /// Use the library at `dir` (`None`: the default) from now on.
    pub(super) fn use_library(&mut self, dir: Option<PathBuf>) {
        // The default chosen by name is the default.
        self.library_dir = dir.filter(|d| Some(d) != library::default_location().as_ref());
        self.read_library();
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

    /// The move's outcome, once it is done.
    pub(super) fn poll_library_move(&mut self, ctx: &egui::Context) {
        let Some((to, rx)) = &self.library_moving else {
            return;
        };
        let to = to.clone();
        match rx.try_recv() {
            Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
            Ok(Ok(left)) => {
                // The whole library is at `to` either way, so it is used there.
                self.library_moving = None;
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
