//! Is there a newer release? One HTTPS request to GitHub's "latest release"
//! endpoint for this repository, compared with the running version.
//!
//! The request carries no account, token or identifier; GitHub sees the
//! address it comes from, as with any download. Nothing is downloaded or
//! installed: a newer release is reported with the page to download it
//! from. Drafts and pre-releases are never "latest", so a release is
//! offered only once a maintainer has published it.
//!
//! Version handling is always compiled; the request itself needs the
//! `update-check` feature, so the engine stays free of networking unless a
//! program asks for it.

use serde::{Deserialize, Serialize};

/// GitHub's API for the newest published release of this repository.
pub const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/peterhudson/timegrapher/releases/latest";

/// The page listing every release, for links when the check cannot run.
pub const RELEASES_PAGE: &str = "https://github.com/peterhudson/timegrapher/releases";

/// A published release, as much of it as the check uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// The version without the leading "v", e.g. "0.2.0".
    pub version: String,
    /// The release's page, where its downloads are.
    pub url: String,
    /// When it was published (RFC 3339), if GitHub said.
    pub published_at: Option<String>,
    /// The release notes (Markdown), possibly empty.
    pub notes: String,
}

/// The result of a check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// The running version.
    pub current: String,
    /// The newest published release.
    pub latest: Release,
    /// True when `latest` is newer than `current`.
    pub update_available: bool,
}

/// A version as (major, minor, patch, pre-release). Accepts a leading "v"
/// and ignores build metadata after "+". A missing minor or patch counts as
/// 0.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let s = s.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let s = s.split('+').next()?;
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
        Some(_) => return None,
        None => (s, None),
    };
    let mut parts = core.split('.');
    let mut num = || -> Option<u64> {
        match parts.next() {
            Some(p) => p.parse().ok(),
            None => Some(0),
        }
    };
    let v = (num()?, num()?, num()?);
    if parts.next().is_some() {
        return None;
    }
    Some((v.0, v.1, v.2, pre))
}

/// True when `latest` is a newer version than `current`. A pre-release
/// sorts before its release (0.2.0-rc.1 < 0.2.0); two pre-releases of the
/// same version compare as text. Unreadable versions are never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (Some(l), Some(c)) = (parse_version(latest), parse_version(current)) else {
        return false;
    };
    let (lt, ct) = ((l.0, l.1, l.2), (c.0, c.1, c.2));
    if lt != ct {
        return lt > ct;
    }
    match (&l.3, &c.3) {
        (None, Some(_)) => true,
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Reads GitHub's release JSON.
pub fn parse_release(json: &str) -> Result<Release, String> {
    #[derive(Deserialize)]
    struct Api {
        tag_name: String,
        html_url: String,
        published_at: Option<String>,
        body: Option<String>,
    }
    let a: Api = serde_json::from_str(json).map_err(|e| format!("unexpected reply: {e}"))?;
    let version = a
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&a.tag_name)
        .to_string();
    if parse_version(&version).is_none() {
        return Err(format!(
            "the latest release has no version number: {}",
            a.tag_name
        ));
    }
    Ok(Release {
        version,
        url: a.html_url,
        published_at: a.published_at,
        notes: a.body.unwrap_or_default(),
    })
}

/// Compares a release with the running version.
pub fn compare(current: &str, latest: Release) -> Check {
    Check {
        current: current.to_string(),
        update_available: is_newer(&latest.version, current),
        latest,
    }
}

/// The reason `latest_release` and `check` give while nothing is published.
pub const NO_RELEASE: &str = "no release has been published yet";

/// Asks GitHub for the newest published release. `user_agent` names the
/// program asking (GitHub requires one), e.g. "timegrapher-app/0.1.0".
/// Gives up after `timeout`. While nothing is published it gives the
/// [`NO_RELEASE`] error; [`published_release`] gives `None` instead.
#[cfg(feature = "update-check")]
pub fn latest_release(user_agent: &str, timeout: std::time::Duration) -> Result<Release, String> {
    published_release(user_agent, timeout)?.ok_or_else(|| NO_RELEASE.to_string())
}

/// As [`latest_release`], but `None` while no release has been published:
/// nothing to offer is an answer, not a failure.
#[cfg(feature = "update-check")]
pub fn published_release(
    user_agent: &str,
    timeout: std::time::Duration,
) -> Result<Option<Release>, String> {
    use ureq::tls::{RootCerts, TlsConfig};
    // The system's own certificate store, so networks that inspect HTTPS
    // with their own authority (offices, schools) work as in a browser.
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent
        .get(LATEST_RELEASE_API)
        .header("User-Agent", user_agent)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("could not reach GitHub: {e}"))?;
    let status = resp.status().as_u16();
    match status {
        200 => {}
        404 => return Ok(None),
        403 | 429 => {
            return Err("GitHub is limiting requests from this address; try again later".into())
        }
        s => return Err(format!("GitHub answered with status {s}")),
    }
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("could not read GitHub's reply: {e}"))?;
    parse_release(&body).map(Some)
}

/// Checks whether a newer release than `current` is published.
#[cfg(feature = "update-check")]
pub fn check(
    current: &str,
    user_agent: &str,
    timeout: std::time::Duration,
) -> Result<Check, String> {
    Ok(compare(current, latest_release(user_agent, timeout)?))
}

/// As [`check`], but `None` while no release has been published.
#[cfg(feature = "update-check")]
pub fn check_published(
    current: &str,
    user_agent: &str,
    timeout: std::time::Duration,
) -> Result<Option<Check>, String> {
    Ok(published_release(user_agent, timeout)?.map(|r| compare(current, r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3, None)));
        assert_eq!(parse_version("0.1"), Some((0, 1, 0, None)));
        assert_eq!(
            parse_version("0.2.0-rc.1+build5"),
            Some((0, 2, 0, Some("rc.1".into())))
        );
        assert_eq!(parse_version("one"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2.3-"), None);
    }

    #[test]
    fn newer() {
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.2.0", "0.2.0-rc.1"));
        assert!(!is_newer("0.2.0-rc.1", "0.2.0"));
        assert!(is_newer("0.2.0-rc.2", "0.2.0-rc.1"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn github_reply() {
        let json = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/peterhudson/timegrapher/releases/tag/v0.2.0",
            "published_at":"2026-11-01T10:00:00Z","body":"Fixes.","draft":false,"prerelease":false}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.notes, "Fixes.");
        let c = compare("0.1.0", r.clone());
        assert!(c.update_available);
        assert!(!compare("0.2.0", r).update_available);
        assert!(parse_release(r#"{"tag_name":"latest","html_url":"x"}"#).is_err());
        assert!(parse_release("not json").is_err());
    }
}
