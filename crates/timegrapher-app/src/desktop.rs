//! Linux only: an entry in the applications menu, with the app's icon, so
//! the desktop can show the icon on the window and in the task bar. Under
//! Wayland a window names its app (its app id, "timegrapher") and the
//! desktop looks that name up among the menu's entries; with no entry it
//! draws a generic icon.

use std::path::PathBuf;

/// The artwork the release packages use.
const ICON: &[u8] = include_bytes!("../../../packaging/icon.png");

/// Write `timegrapher.desktop` and the icon under the user's data folder
/// (`$XDG_DATA_HOME`, or `~/.local/share`), pointing at this executable,
/// or at the AppImage it runs from. Returns the entry's path.
pub fn install() -> std::io::Result<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| std::io::Error::other("no home folder"))?;
    let running = std::env::current_exe()?;
    let exe = appimage(
        std::env::var_os("APPIMAGE"),
        std::env::var_os("APPDIR"),
        &running,
    )
    .unwrap_or(running);
    let icons = data.join("icons/hicolor/512x512/apps");
    std::fs::create_dir_all(&icons)?;
    std::fs::write(icons.join("timegrapher.png"), ICON)?;
    let apps = data.join("applications");
    std::fs::create_dir_all(&apps)?;
    let entry = apps.join("timegrapher.desktop");
    std::fs::write(&entry, entry_text(&exe.display().to_string()))?;
    Ok(entry)
}

/// The AppImage file this app runs from, if any. Inside an AppImage the
/// executable sits in a temporary mount that goes when the app quits, so
/// the menu entry must name the AppImage itself, which its runtime puts in
/// `$APPIMAGE`, and mounts at `$APPDIR`. Only when this executable is
/// under `$APPDIR`: an AppImage passes both on to programs it starts.
fn appimage(
    image: Option<std::ffi::OsString>,
    dir: Option<std::ffi::OsString>,
    exe: &std::path::Path,
) -> Option<PathBuf> {
    let dir = PathBuf::from(dir?);
    if !dir.is_absolute() || !exe.starts_with(&dir) {
        return None;
    }
    image
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
}

/// The menu entry for an executable at `exe`.
fn entry_text(exe: &str) -> String {
    // Exec quotes the path, so spaces in it are kept.
    let quoted = format!("\"{}\"", exe.replace('\\', "\\\\").replace('"', "\\\""));
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Timegrapher\n\
         Comment=Measure a mechanical watch's rate, amplitude and beat error from a microphone\n\
         Exec={quoted} %f\n\
         Icon=timegrapher\n\
         StartupWMClass=timegrapher\n\
         Categories=Utility;Science;\n\
         Terminal=false\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_runs_this_executable_and_names_the_icon() {
        let e = entry_text("/home/a b/timegrapher-app");
        assert!(e.contains("Exec=\"/home/a b/timegrapher-app\" %f"), "{e}");
        assert!(e.contains("Icon=timegrapher\n"));
        assert!(e.contains("StartupWMClass=timegrapher\n"));
    }

    #[test]
    fn an_appimage_is_used_only_when_it_is_a_file_and_runs_this() {
        let file = std::env::current_exe().unwrap();
        // Absolute on every system the tests run on.
        let mount = file.parent().unwrap().join(".mount_Timegr");
        let mount = mount.as_path();
        let exe = mount.join("usr").join("bin").join("timegrapher-app");
        let elsewhere = file.parent().unwrap().join("other").join("x");
        let some = |p: &std::path::Path| Some(p.as_os_str().to_owned());
        assert_eq!(appimage(some(&file), some(mount), &exe), Some(file.clone()));
        assert_eq!(appimage(None, some(mount), &exe), None);
        assert_eq!(appimage(some(&file), None, &exe), None);
        // Started by an AppImage, not run from one.
        assert_eq!(appimage(some(&file), some(mount), &elsewhere), None);
        assert_eq!(
            appimage(Some("Timegrapher.AppImage".into()), some(mount), &exe),
            None
        );
        assert_eq!(
            appimage(
                Some("/no/such/Timegrapher.AppImage".into()),
                some(mount),
                &exe
            ),
            None
        );
    }
}
