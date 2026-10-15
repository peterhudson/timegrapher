//! Recording safely when a chat app retries or overlaps calls: one tool on
//! the microphone at a time, and a recording retried with the same
//! `take_id` answered from the take already made rather than recorded again.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{Map, Value};

static MIC_BUSY: AtomicBool = AtomicBool::new(false);

/// Said when a second call wants the microphone.
pub const BUSY: &str = "The microphone is in use by another recording or microphone check. \
    Wait for that call to finish, then try again. Nothing was recorded by this call.";

/// Holds the microphone for one call; dropping it frees it, however the
/// call ends.
pub struct Mic(());

impl Drop for Mic {
    fn drop(&mut self) {
        MIC_BUSY.store(false, Ordering::Release);
    }
}

/// The microphone, unless another call has it.
pub fn claim_mic() -> Option<Mic> {
    MIC_BUSY
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
        .then_some(Mic(()))
}

/// A take ID, its resolved native destination, and the exact position.
pub type Key = (String, PathBuf, String);

static TAKES: Mutex<Option<HashMap<Key, Value>>> = Mutex::new(None);

/// The call's take, if it named one.
pub fn key(args: &Map<String, Value>, folder: &Path) -> Option<Key> {
    let text = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    Some((
        text(crate::tools::TAKE_ID)?,
        folder.to_path_buf(),
        text("position")?,
    ))
}

/// Resolve the recording folder before checking the retry cache or spawning
/// the CLI. The child receives this same native path, including missing tails.
pub fn destination(args: &Map<String, Value>) -> Result<PathBuf, String> {
    let folder = args
        .get("folder")
        .and_then(Value::as_str)
        .ok_or("record_position: folder must be a path")?;
    let path = crate::tools::expand_home_path(folder)?;
    resolve_destination(&path).map_err(|e| {
        format!(
            "record_position: cannot resolve folder {}: {e}",
            path.display()
        )
    })
}

fn resolve_destination(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("destination must be an absolute path".into());
    }
    let root = path.ancestors().last().ok_or("destination has no root")?;
    let mut resolved = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                let candidate = resolved.join(name);
                match std::fs::symlink_metadata(&candidate) {
                    Ok(_) => {
                        // Resolve links before a subsequent '..'. Collapsing
                        // the input lexically first would choose another folder.
                        resolved = std::fs::canonicalize(&candidate)
                            .map_err(|e| format!("{}: {e}", candidate.display()))?;
                        if !resolved.is_dir() {
                            return Err(format!("{} is not a directory", candidate.display()));
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        resolved = candidate;
                    }
                    Err(e) => return Err(format!("{}: {e}", candidate.display())),
                }
            }
        }
    }
    Ok(resolved)
}

/// The result of the take, if an earlier call recorded it, marked as such.
pub fn recorded(key: &Key) -> Option<Value> {
    let takes = TAKES.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = takes.as_ref()?.get(key)?.clone();
    if let Some(text) = r["content"][0]["text"].as_str() {
        r["content"][0]["text"] = Value::String(format!(
            "{text}\n\nThis take was recorded by an earlier call with take_id {:?}; \
             nothing was recorded again.",
            key.0
        ));
    }
    Some(r)
}

/// Keep a take's result for a retry.
pub fn remember(key: Key, result: &Value) {
    let mut takes = TAKES.lock().unwrap_or_else(|e| e.into_inner());
    takes
        .get_or_insert_with(HashMap::new)
        .insert(key, result.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let folder =
                std::env::temp_dir().join(format!("tg-retry-paths-{}-{id}", std::process::id()));
            std::fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn destination_identity_survives_creation_and_dot_components() {
        let folder = Folder::new();
        let root = std::fs::canonicalize(&folder.0).unwrap();
        let requested = folder.0.join("new/deeper");
        let identity = resolve_destination(&requested).unwrap();
        assert_eq!(identity, root.join("new/deeper"));
        assert!(
            !requested.exists(),
            "identity lookup must not create output"
        );
        std::fs::create_dir(folder.0.join("existing")).unwrap();
        assert_eq!(
            resolve_destination(&folder.0.join("existing/../new/./deeper")).unwrap(),
            identity
        );
        assert_eq!(
            resolve_destination(&folder.0.join("missing/../new/deeper")).unwrap(),
            identity
        );
        std::fs::create_dir_all(&identity).unwrap();
        assert_eq!(resolve_destination(&requested).unwrap(), identity);
        assert_eq!(resolve_destination(&identity).unwrap(), identity);
        assert!(!folder.0.join("missing").exists());
    }

    #[test]
    fn invalid_destinations_are_errors_rather_than_uncached_calls() {
        let folder = Folder::new();
        assert!(resolve_destination(Path::new("relative/path")).is_err());
        std::fs::write(folder.0.join("file"), []).unwrap();
        for path in [
            folder.0.join("file"),
            folder.0.join("file/child"),
            folder.0.join("file/../other"),
        ] {
            let error = resolve_destination(&path).unwrap_err();
            assert!(error.contains("not a directory"), "{error}");
        }
        let args = json!({"folder": "relative/path", "position":"DU", "take_id":"id"});
        assert!(destination(args.as_object().unwrap())
            .unwrap_err()
            .contains("not a full path"));
    }

    #[test]
    fn native_identity_does_not_merge_distinct_takes_or_positions() {
        let folder = Folder::new();
        let a = resolve_destination(&folder.0.join("a")).unwrap();
        let b = resolve_destination(&folder.0.join("b")).unwrap();
        let args = json!({"take_id":"one", "position":"DU"});
        let original = key(args.as_object().unwrap(), &a).unwrap();
        assert_ne!(original, key(args.as_object().unwrap(), &b).unwrap());
        for other in [
            json!({"take_id":"two", "position":"DU"}),
            json!({"take_id":"one", "position":"DD"}),
        ] {
            assert_ne!(original, key(other.as_object().unwrap(), &a).unwrap());
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_resolution_precedes_parent_traversal_and_rejects_dangling_links() {
        use std::os::unix::fs::symlink;
        let folder = Folder::new();
        std::fs::create_dir_all(folder.0.join("physical/deep")).unwrap();
        symlink(folder.0.join("physical/deep"), folder.0.join("shortcut")).unwrap();
        let requested = folder.0.join("shortcut/../new");
        let identity = resolve_destination(&requested).unwrap();
        assert_eq!(
            identity,
            std::fs::canonicalize(folder.0.join("physical"))
                .unwrap()
                .join("new")
        );
        assert_ne!(
            identity,
            resolve_destination(&folder.0.join("new")).unwrap()
        );
        std::fs::create_dir_all(&identity).unwrap();
        assert_eq!(identity, std::fs::canonicalize(&requested).unwrap());
        symlink(folder.0.join("not-created"), folder.0.join("dangling")).unwrap();
        assert!(resolve_destination(&folder.0.join("dangling/child")).is_err());
        symlink("loop", folder.0.join("loop")).unwrap();
        assert!(resolve_destination(&folder.0.join("loop")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn native_key_and_os_arguments_preserve_bytes_without_filesystem_creation() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let folder = Folder::new();
        let native = folder
            .0
            .join(std::ffi::OsString::from_vec(b"native-\xff".to_vec()));
        let args = json!({"take_id":"one", "position":"DU"});
        let native_key = key(args.as_object().unwrap(), &native).unwrap();
        assert_eq!(native_key.1, native);
        let argument = native_key.1.clone().into_os_string();
        assert_eq!(argument.as_bytes(), native.as_os_str().as_bytes());
        let lossy = PathBuf::from(native.to_string_lossy().into_owned());
        assert_ne!(native_key, key(args.as_object().unwrap(), &lossy).unwrap());
    }

    // Raw-byte directory names are exercised on Linux; this local macOS
    // environment rejects fixture creation before the resolver is called.
    #[cfg(target_os = "linux")]
    #[test]
    fn non_utf8_target_identity_is_native_and_distinct_from_lossy_spelling() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::symlink;
        let folder = Folder::new();
        let native = folder
            .0
            .join(std::ffi::OsString::from_vec(b"native-\xff".to_vec()));
        let lossy = folder.0.join("native-�");
        std::fs::create_dir(&native).unwrap();
        std::fs::create_dir(&lossy).unwrap();
        symlink(&native, folder.0.join("utf8-alias")).unwrap();
        let identity = resolve_destination(&folder.0.join("utf8-alias/new")).unwrap();
        assert_eq!(
            identity,
            std::fs::canonicalize(&native).unwrap().join("new")
        );
        assert_ne!(identity, resolve_destination(&lossy.join("new")).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_and_extended_prefix_aliases_share_existing_identity() {
        let folder = Folder::new();
        std::fs::create_dir(folder.0.join("MixedCase")).unwrap();
        let expected = std::fs::canonicalize(folder.0.join("MixedCase")).unwrap();
        assert_eq!(
            resolve_destination(&folder.0.join("mixedcase")).unwrap(),
            expected
        );
        assert_eq!(resolve_destination(&expected).unwrap(), expected);
        assert_eq!(
            resolve_destination(&expected.join("new")).unwrap(),
            expected.join("new")
        );
    }

    #[test]
    fn one_call_at_a_time_has_the_mic() {
        let first = claim_mic().expect("free");
        assert!(claim_mic().is_none());
        drop(first);
        assert!(claim_mic().is_some());
    }

    #[test]
    fn a_retried_take_comes_back_marked() {
        let key = |args: &Map<String, Value>| super::key(args, Path::new("/w"));
        let args = |id: &str, pos: &str| {
            json!({"folder": "/w", "position": pos, "take_id": id})
                .as_object()
                .unwrap()
                .clone()
        };
        let k = key(&args("t-1", "DU")).unwrap();
        assert!(recorded(&k).is_none());
        remember(
            k.clone(),
            &json!({"content": [{"type": "text", "text": "{}"}]}),
        );
        let again = recorded(&k).unwrap();
        assert!(again["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("nothing was recorded again"));
        // The same name in another position is another take.
        assert!(recorded(&key(&args("t-1", "DD")).unwrap()).is_none());
        let no_id = json!({"folder": "/w", "position": "DU"});
        assert!(key(no_id.as_object().unwrap()).is_none());
    }
}
