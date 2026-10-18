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
    let exe = match appimage(std::env::var_os("APPIMAGE")) {
        Some(file) => file,
        None => std::env::current_exe()?,
    };
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
/// `$APPIMAGE`.
fn appimage(var: Option<std::ffi::OsString>) -> Option<PathBuf> {
    var.map(PathBuf::from)
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
    fn an_appimage_is_used_only_when_it_is_a_file() {
        let file = std::env::current_exe().unwrap();
        assert_eq!(appimage(Some(file.clone().into())), Some(file));
        assert_eq!(appimage(None), None);
        assert_eq!(appimage(Some("Timegrapher.AppImage".into())), None);
        assert_eq!(appimage(Some("/no/such/Timegrapher.AppImage".into())), None);
    }
}
