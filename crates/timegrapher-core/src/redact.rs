//! What an issue report keeps of the computer it was made on, for the app's
//! Report an Issue and `timegrapher report` alike: the home folder, user
//! name and host name taken out of every text it holds, the OS's version,
//! and the week's logs. One copy, so a privacy fix reaches both.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Removes the home folder, user name and host name from text, counting
/// what it removed.
pub struct Redactor {
    /// The home folder, as written and as JSON writes a Windows path.
    pub home: Vec<String>,
    pub user: Option<String>,
    pub host: Option<String>,
    /// Removed: home folders, user names, host names.
    pub counts: [usize; 3],
}

impl Redactor {
    /// For the computer this runs on, from its environment.
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = var("HOME").or_else(|| var("USERPROFILE"));
        let mut homes = Vec::new();
        if let Some(h) = home {
            let h = h.trim_end_matches(['/', '\\']).to_string();
            // As written, and as JSON writes a Windows path.
            if h.contains('\\') {
                homes.push(h.replace('\\', "\\\\"));
            }
            homes.push(h);
        }
        let host = var("HOSTNAME").or_else(|| var("COMPUTERNAME")).or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
        Redactor {
            home: homes,
            user: var("USER").or_else(|| var("USERNAME")),
            host,
            counts: [0; 3],
        }
    }

    /// `text` with the home folder written `~`, the user name `<user>`
    /// and the host name `<host>` (each a whole word).
    pub fn apply(&mut self, text: &str) -> String {
        let mut s = text.to_string();
        for h in &self.home {
            if h.len() > 1 {
                self.counts[0] += s.matches(h.as_str()).count();
                s = s.replace(h.as_str(), "~");
            }
        }
        if let Some(u) = &self.user {
            let (t, n) = replace_word(&s, u, "<user>");
            s = t;
            self.counts[1] += n;
        }
        if let Some(h) = &self.host {
            let (t, n) = replace_word(&s, h, "<host>");
            s = t;
            self.counts[2] += n;
        }
        s
    }

    /// What was removed, by kind, as the report lists it.
    pub fn summary(&self) -> Value {
        json!({
            "home_folder": self.counts[0],
            "user_name": self.counts[1],
            "host_name": self.counts[2],
        })
    }
}

/// Replace whole-word occurrences of `word` (at least two characters).
pub fn replace_word(s: &str, word: &str, with: &str) -> (String, usize) {
    if word.chars().count() < 2 {
        return (s.to_string(), 0);
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(s.len());
    let mut n = 0;
    let mut rest = s;
    while let Some(i) = rest.find(word) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + word.len()..].chars().next();
        let whole = !before.is_some_and(is_word) && !after.is_some_and(is_word);
        out.push_str(&rest[..i]);
        if whole {
            out.push_str(with);
            n += 1;
        } else {
            out.push_str(word);
        }
        rest = &rest[i + word.len()..];
    }
    out.push_str(rest);
    (out, n)
}

/// The OS's own name for its version, where it is easy to ask.
pub fn os_version() -> Option<String> {
    if cfg!(target_os = "linux") {
        let t = std::fs::read_to_string("/etc/os-release").ok()?;
        t.lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_string())
    } else if cfg!(target_os = "macos") {
        let o = Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        Some(format!(
            "macOS {}",
            String::from_utf8_lossy(&o.stdout).trim()
        ))
    } else if cfg!(target_os = "windows") {
        let o = Command::new("cmd").args(["/C", "ver"]).output().ok()?;
        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        None
    }
}

/// Files modified in the last week in `dir` whose names pass `keep`.
pub fn recent_files(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let week = SystemTime::now() - Duration::from_secs(7 * 86400);
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| keep(&e.file_name().to_string_lossy()))
        .filter(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t >= week)
        })
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_takes_whole_words() {
        let mut r = Redactor {
            home: vec!["/home/ann".into()],
            user: Some("ann".into()),
            host: Some("bench".into()),
            counts: [0; 3],
        };
        let t = r.apply("/home/ann/takes/a.flac by ann on bench; annual benchmark");
        assert_eq!(t, "~/takes/a.flac by <user> on <host>; annual benchmark");
        assert_eq!(
            r.summary(),
            json!({"home_folder": 1, "user_name": 1, "host_name": 1})
        );
    }

    #[test]
    fn windows_home_in_json() {
        let mut r = Redactor {
            home: vec!["C:\\\\Users\\\\Ann".into(), "C:\\Users\\Ann".into()],
            user: None,
            host: None,
            counts: [0; 3],
        };
        let j = serde_json::to_string(&json!({"path": "C:\\Users\\Ann\\x.wav"})).unwrap();
        assert_eq!(r.apply(&j), r#"{"path":"~\\x.wav"}"#);
    }

    #[test]
    fn a_name_inside_a_longer_word_stays() {
        assert_eq!(
            replace_word("ann's annex", "ann", "<user>"),
            ("<user>'s annex".to_string(), 1)
        );
        // A one-letter name is never taken out.
        assert_eq!(replace_word("a b", "a", "<user>").1, 0);
    }
}
