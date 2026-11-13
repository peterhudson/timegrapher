//! `timegrapher watches new|edit|add-take|move-take|keep-analysis-only|
//! delete-take|add-photos|delete-photo`: the changes the app's Watches
//! view makes, from the command line, each by the same function of core's
//! `library` (and `analysis_take` for Keep Analysis Only) the app calls.
//!
//! Each takes the library's lock, as the app does, so it is refused while
//! a copy of the app has the library open: two programs never write the
//! same watch at once. `add-take` alone works while the app is open
//! ([`Library::open_to_add`]): the take is put together beside its place
//! and moved in whole, a new folder nothing else writes, and the app lists
//! it the next time it reads the library (at its start, when its window
//! comes back to the front, or after its own next save).

use crate::output;
use clap::Subcommand;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use timegrapher_core::analysis_take;
use timegrapher_core::calibre_store::{self, UserCalibres};
use timegrapher_core::library::{self, FromCalibre, Library, Watch, WatchEntry};
use timegrapher_core::{input_domain, take};

/// The changes, as subcommands of `watches`.
#[derive(Subcommand)]
pub enum Change {
    /// Add a watch: its folder in the library, with its details. Only the
    /// name is needed. The first watch makes the library, in a folder
    /// that is missing or empty, as in the app. A calibre from the table gives the lift angle and
    /// beat rate it has, as in the app.
    New {
        name: String,
        #[command(flatten)]
        details: Details,
    },
    /// Change a watch's details (found by its name, ignoring case). Only
    /// what is given changes; its folder keeps its name.
    Edit {
        name: String,
        /// A new name for the watch.
        #[arg(long)]
        rename: Option<String>,
        #[command(flatten)]
        details: Details,
        /// Remove a field the watch was given (repeatable).
        #[arg(long = "remove-field", value_name = "LABEL")]
        remove_field: Vec<String>,
    },
    /// Add a take folder from outside the library to a watch: copied (the
    /// original stays), or moved with --move. Put together beside its
    /// place and moved in whole, so a copy cut short is never a take.
    /// Works while the app is open; the app lists it the next time it
    /// reads the library.
    AddTake {
        /// The watch, by its name.
        watch: String,
        /// The take folder: a session.toml or session.json, with its sound.
        folder: PathBuf,
        /// Move the folder in rather than copying it.
        #[arg(long = "move")]
        move_it: bool,
    },
    /// Move a take of the library (its folder, as `watches show` gives it)
    /// to another watch; a take in Unsorted is filed under the watch so.
    MoveTake {
        take: PathBuf,
        /// The watch it goes to, by its name.
        #[arg(long)]
        to: String,
    },
    /// Read a take's sound once more, keep its analysis, check it reads
    /// back, and only then delete the sound, keeping every other file in
    /// the take. Takes about as long as reading the take twice.
    KeepAnalysisOnly { take: PathBuf },
    /// Delete a take of the library and everything in it. It can't be
    /// undone, so --yes is required.
    DeleteTake {
        take: PathBuf,
        #[arg(long)]
        yes: bool,
    },
    /// Copy photos (JPEG, PNG, HEIC or TIFF) into a watch's photos.
    AddPhotos {
        watch: String,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Delete one of a watch's photos (its file, as in the watch's
    /// photos folder). It can't be undone, so --yes is required.
    DeletePhoto {
        photo: PathBuf,
        #[arg(long)]
        yes: bool,
    },
}

/// A watch's details, for `new` and `edit`.
#[derive(clap::Args)]
pub struct Details {
    /// The movement, from the calibre table (`timegrapher calibres
    /// --search`); for `edit`, "" removes it.
    #[arg(long)]
    calibre: Option<String>,
    /// Beat rate in beats per hour, where the calibre gives none.
    #[arg(long)]
    bph: Option<u64>,
    /// Lift angle in degrees, where the calibre gives none.
    #[arg(long)]
    lift: Option<f64>,
    /// Notes, as typed; for `edit`, "" removes them.
    #[arg(long)]
    notes: Option<String>,
    /// A field of the watch's, as LABEL=VALUE (repeatable): one it has is
    /// set, one it hasn't is added as text, as Add Field does.
    #[arg(long = "field", value_name = "LABEL=VALUE")]
    fields: Vec<String>,
}

/// The library's folder: `library`, or `~/Timegrapher Data`, absolute,
/// so every folder said is one to pass on.
fn root(library: Option<&Path>) -> Result<PathBuf, String> {
    let root = library
        .map(Path::to_path_buf)
        .or_else(library::default_location)
        .ok_or("there is no home folder: give the library with --library")?;
    Ok(std::path::absolute(&root).unwrap_or(root))
}

/// The library at [`root`], with its lock unless `to_add`.
fn open(library: Option<&Path>, to_add: bool) -> Result<Library, String> {
    let root = root(library)?;
    if !Library::is_library(&root) {
        return Err(format!(
            "{} is not a Timegrapher library (it has no {}); the app makes one with its \
             first watch, and --library names one elsewhere",
            root.display(),
            library::LIBRARY_FILE
        ));
    }
    let lib = if to_add {
        Library::open_to_add(&root)?
    } else {
        Library::open(&root)?
    };
    locked(lib, to_add)
}

/// `lib`, unless it couldn't be locked where a change needs the lock.
fn locked(lib: Library, to_add: bool) -> Result<Library, String> {
    match &lib.read_only {
        Some(why) if !to_add => Err(format!(
            "{why}. Close the app and run this again; `watches add-take` works while it is open"
        )),
        _ => Ok(lib),
    }
}

fn watch<'a>(lib: &'a Library, name: &str) -> Result<&'a WatchEntry, String> {
    lib.find(name).ok_or_else(|| {
        format!(
            "there is no watch called \"{}\" in the library",
            name.trim()
        )
    })
}

/// `p` absolute, as the library's own folders are, to find it among them.
fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn user_calibres() -> Result<UserCalibres, String> {
    match calibre_store::default_path() {
        Some(p) => UserCalibres::load(&p),
        None => Ok(UserCalibres::default()),
    }
}

/// `d` applied to `w`: the calibre first, as the app's form takes it, its
/// lift angle and beat rate then the calibre's where it gives them.
fn apply(w: &mut Watch, d: &Details) -> Result<(), String> {
    if let Some(name) = d.calibre.as_deref() {
        if name.trim().is_empty() {
            w.calibre = None;
        } else {
            let c = take::calibre_named(name, &user_calibres()?).ok_or_else(|| {
                format!(
                    "\"{}\" is not in the calibre table: `timegrapher calibres --search` finds \
                     its name there",
                    name.trim()
                )
            })?;
            w.pick_calibre(&c.name, FromCalibre::of(&c));
        }
    }
    let given = match w.calibre.as_deref() {
        Some(n) => take::calibre_named(n, &user_calibres()?)
            .map(|c| FromCalibre::of(&c))
            .unwrap_or_default(),
        None => FromCalibre::default(),
    };
    // Core says why; the flag to leave out is the command line's.
    if let Some(b) = d.bph {
        w.set_bph(Some(input_domain::bph(b)?), given)
            .map_err(|e| format!("{e}, or leave out --bph"))?;
    }
    if let Some(l) = d.lift {
        w.set_lift(l, given)
            .map_err(|e| format!("{e}, or leave out --lift"))?;
    }
    if let Some(n) = &d.notes {
        w.notes = (!n.is_empty()).then(|| n.clone());
    }
    for f in &d.fields {
        let (label, value) = f
            .split_once('=')
            .ok_or_else(|| format!("--field {f}: give it as LABEL=VALUE"))?;
        w.set_field(label, value)?;
    }
    Ok(())
}

/// What a change did, printed as text or as `timegrapher.watches-change/1`.
struct Done {
    action: &'static str,
    /// The watch changed or added to, after the change.
    watch: Option<Value>,
    /// The take's folder after the change (gone, for `delete-take`).
    take: Option<PathBuf>,
    photos: Vec<PathBuf>,
    /// Said where the change is made but something was left behind.
    left: Option<String>,
    text: String,
}

fn watch_json(e: &WatchEntry) -> Value {
    json!({
        "id": e.watch.id,
        "name": e.watch.name,
        "folder": e.dir.display().to_string(),
    })
}

/// The watch of id `id` as the library now reads it.
fn watch_now(lib: &Library, id: &str) -> Option<Value> {
    lib.watches
        .iter()
        .find(|w| w.watch.id == id)
        .map(watch_json)
}

pub fn run(c: &Change, library: Option<&Path>, json: bool) -> Result<(), String> {
    let done = change(c, library)?;
    if let Some(why) = &done.left {
        eprintln!("{why}");
    }
    if !json {
        println!("{}", done.text);
        return Ok(());
    }
    let input = json!({ "library": library.map(|l| l.display().to_string()) });
    let body = json!({
        "action": done.action,
        "watch": done.watch,
        "take": done.take.map(|t| t.display().to_string()),
        "photos": done.photos.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "left": done.left,
        "caveats": [],
    });
    output::print("watches-change", input, &body)
}

fn change(c: &Change, library: Option<&Path>) -> Result<Done, String> {
    match c {
        Change::New { name, details } => new(library, name, details),
        Change::Edit {
            name,
            rename,
            details,
            remove_field,
        } => edit(library, name, rename.as_deref(), details, remove_field),
        Change::AddTake {
            watch,
            folder,
            move_it,
        } => add_take(library, watch, folder, *move_it),
        Change::MoveTake { take, to } => move_take(library, take, to),
        Change::KeepAnalysisOnly { take } => keep_analysis_only(library, take),
        Change::DeleteTake { take, yes } => delete_take(library, take, *yes),
        Change::AddPhotos { watch, files } => add_photos(library, watch, files),
        Change::DeletePhoto { photo, yes } => delete_photo(library, photo, *yes),
    }
}

fn done(action: &'static str, text: String) -> Done {
    Done {
        action,
        watch: None,
        take: None,
        photos: Vec::new(),
        left: None,
        text,
    }
}

fn new(library: Option<&Path>, name: &str, d: &Details) -> Result<Done, String> {
    let mut lib = locked(Library::open_or_create(&root(library)?)?, false)?;
    let mut w = Watch::new(name);
    apply(&mut w, d)?;
    let dir = lib.add(&w)?;
    Ok(Done {
        watch: watch_now(&lib, &w.id),
        ..done("new", format!("added \"{}\" in {}", w.name, dir.display()))
    })
}

fn edit(
    library: Option<&Path>,
    name: &str,
    rename: Option<&str>,
    d: &Details,
    remove: &[String],
) -> Result<Done, String> {
    let mut lib = open(library, false)?;
    let mut w = watch(&lib, name)?.watch.clone();
    if let Some(n) = rename {
        w.name = n.trim().to_string();
    }
    for label in remove {
        w.remove_field(label)?;
    }
    apply(&mut w, d)?;
    lib.update(&w)?;
    Ok(Done {
        watch: watch_now(&lib, &w.id),
        ..done("edit", format!("saved \"{}\"", w.name))
    })
}

fn add_take(
    library: Option<&Path>,
    name: &str,
    from: &Path,
    move_it: bool,
) -> Result<Done, String> {
    let mut lib = open(library, true)?;
    let id = watch(&lib, name)?.watch.id.clone();
    let (to, left) = lib.add_take(&absolute(from), &id, !move_it)?;
    let how = if move_it { "moved" } else { "copied" };
    Ok(Done {
        watch: watch_now(&lib, &id),
        take: Some(to.clone()),
        left,
        ..done("add-take", format!("{how} into {}", to.display()))
    })
}

fn move_take(library: Option<&Path>, take: &Path, to: &str) -> Result<Done, String> {
    let mut lib = open(library, false)?;
    let id = watch(&lib, to)?.watch.id.clone();
    let moved = lib.move_take(&absolute(take), &id)?;
    Ok(Done {
        watch: watch_now(&lib, &id),
        take: Some(moved.clone()),
        ..done("move-take", format!("moved to {}", moved.display()))
    })
}

/// A take of the library, where this copy may change it: `Err` naming it
/// otherwise, before anything is read.
fn take_of(lib: &Library, take: &Path) -> Result<Option<Value>, String> {
    if let Some(e) = lib.watch_of(take) {
        return Ok(Some(watch_json(e)));
    }
    if lib.is_unsorted(take) && take.is_dir() {
        return Ok(None);
    }
    Err(format!("{} is not a take of these watches", take.display()))
}

fn keep_analysis_only(library: Option<&Path>, take: &Path) -> Result<Done, String> {
    let mut lib = open(library, false)?;
    let take = absolute(take);
    let watch = take_of(&lib, &take)?;
    let plan = analysis_take::plan_keep(&take, &user_calibres()?)
        .map_err(|e| format!("the sound couldn't be removed, so nothing was changed: {e}"))?;
    let before = library::folder_bytes(&take);
    eprintln!(
        "reading {} ({:.0} s of sound, read twice)",
        take.display(),
        plan.total_s
    );
    let kept = analysis_take::write_kept(&take, &plan, output::software(), |_| {})?;
    let left = lib.put_kept(&take, &kept)?;
    let after = library::folder_bytes(&take);
    Ok(Done {
        watch,
        take: Some(take.clone()),
        left,
        ..done(
            "keep-analysis-only",
            format!(
                "kept the analysis of {}: {}, down from {}",
                take.display(),
                library::size_text(after),
                library::size_text(before)
            ),
        )
    })
}

fn delete_take(library: Option<&Path>, take: &Path, yes: bool) -> Result<Done, String> {
    if !yes {
        return Err("deleting a take can't be undone: add --yes to delete it".into());
    }
    let mut lib = open(library, false)?;
    let take = absolute(take);
    let watch = take_of(&lib, &take)?;
    lib.delete_take(&take)?;
    Ok(Done {
        watch,
        take: Some(take.clone()),
        ..done("delete-take", format!("deleted {}", take.display()))
    })
}

fn add_photos(library: Option<&Path>, name: &str, files: &[PathBuf]) -> Result<Done, String> {
    let mut lib = open(library, false)?;
    let id = watch(&lib, name)?.watch.id.clone();
    let files: Vec<PathBuf> = files.iter().map(|f| absolute(f)).collect();
    let added = lib.add_photos(&id, &files)?;
    Ok(Done {
        watch: watch_now(&lib, &id),
        photos: added.clone(),
        ..done(
            "add-photos",
            added
                .iter()
                .map(|p| format!("added {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })
}

fn delete_photo(library: Option<&Path>, photo: &Path, yes: bool) -> Result<Done, String> {
    if !yes {
        return Err("deleting a photo can't be undone: add --yes to delete it".into());
    }
    let mut lib = open(library, false)?;
    let photo = absolute(photo);
    let watch = lib
        .watches
        .iter()
        .find(|w| w.photos.contains(&photo))
        .map(watch_json);
    lib.delete_photo(&photo)?;
    Ok(Done {
        watch,
        photos: vec![photo.clone()],
        ..done("delete-photo", format!("deleted {}", photo.display()))
    })
}
