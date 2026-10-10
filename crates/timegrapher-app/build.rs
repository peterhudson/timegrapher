//! Build information for `--version` and the JSON `software` block: the
//! short git hash, whether the work tree had uncommitted changes, and the
//! build date (UTC). A build from a source tarball, with no git, reports
//! "unknown" for the hash and leaves the dirty flag empty.
//!
//! The script re-runs only when HEAD moves (a commit or a checkout), not on
//! every source edit, so the hash costs no rebuilds. The dirty flag and the
//! date are therefore those of the last time HEAD moved.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

/// Ask cargo to re-run this script when HEAD or the branch it names moves.
fn watch_head() {
    let Some(git_dir) = git(&["rev-parse", "--git-dir"]).map(PathBuf::from) else {
        return;
    };
    let common = git(&["rev-parse", "--git-common-dir"])
        .map(PathBuf::from)
        .unwrap_or_else(|| git_dir.clone());
    let watch = |p: &Path| {
        if p.exists() {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    };
    watch(&git_dir.join("HEAD"));
    if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watch(&common.join(&r));
        watch(&common.join("packed-refs"));
    }
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    watch_head();

    let hash = git(&["rev-parse", "--short=7", "HEAD"]).filter(|h| !h.is_empty());
    let dirty = hash.as_ref().and_then(|_| {
        git(&["status", "--porcelain", "--untracked-files=no"]).map(|s| !s.is_empty())
    });
    // SOURCE_DATE_EPOCH, where set, makes the build reproducible.
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let date = format!("{y:04}-{m:02}-{d:02}");

    let hash = hash.unwrap_or_else(|| "unknown".into());
    let dirty = match dirty {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    };
    let shown = if dirty == "true" {
        format!("{hash}-dirty")
    } else {
        hash.clone()
    };
    println!("cargo:rustc-env=TIMEGRAPHER_GIT_HASH={hash}");
    println!("cargo:rustc-env=TIMEGRAPHER_GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=TIMEGRAPHER_BUILD_DATE={date}");
    println!(
        "cargo:rustc-env=TIMEGRAPHER_LONG_VERSION={} ({shown}, built {date})",
        env!("CARGO_PKG_VERSION")
    );
}
