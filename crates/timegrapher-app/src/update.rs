//! Is a newer Timegrapher published? At start (when Settings allow) and
//! from Check for Updates…, one request to GitHub on a background thread,
//! never the UI's. Nothing downloads or installs itself: a newer release is
//! offered with a button that opens its page in the browser.

use eframe::egui;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use timegrapher_core::update::{self, Check, Release};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct Updates {
    /// A check under way, and whether someone asked for it (its result is
    /// always shown) rather than the one at start (shown only when there is
    /// something newer).
    job: Option<(bool, Receiver<Result<Check, String>>)>,
    /// A newer release to offer, until it is dismissed.
    pub notice: Option<Release>,
}

impl Updates {
    pub fn checking(&self) -> bool {
        self.job.is_some()
    }

    pub fn start(&mut self, ctx: &egui::Context, asked: bool) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let agent = format!("timegrapher-app/{VERSION}");
            let _ = tx.send(update::check(VERSION, &agent, Duration::from_secs(10)));
            ctx.request_repaint();
        });
        self.job = Some((asked, rx));
    }

    /// Take in a finished check. Returns what to say when someone asked.
    pub fn poll(&mut self) -> Option<String> {
        let (asked, rx) = self.job.as_ref()?;
        let asked = *asked;
        let result = rx.try_recv().ok()?;
        self.job = None;
        match result {
            Ok(c) if c.update_available => {
                self.notice = Some(c.latest);
                None
            }
            Ok(c) => asked.then(|| {
                if c.latest.version == c.current {
                    format!("Timegrapher {} is up to date", c.current)
                } else {
                    format!(
                        "Timegrapher {} is up to date (the latest release is {})",
                        c.current, c.latest.version
                    )
                }
            }),
            Err(e) => asked.then(|| format!("Couldn't check for updates: {e}")),
        }
    }
}

/// The page to send someone to when the release has no page of its own.
pub fn page(r: &Release) -> &str {
    if r.url.is_empty() {
        update::RELEASES_PAGE
    } else {
        &r.url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_asked_check_reports_being_up_to_date() {
        let release = Release {
            version: VERSION.into(),
            url: String::new(),
            published_at: None,
            notes: String::new(),
        };
        for asked in [false, true] {
            let (tx, rx) = channel();
            tx.send(Ok(update::compare(VERSION, release.clone())))
                .unwrap();
            let mut u = Updates {
                job: Some((asked, rx)),
                notice: None,
            };
            assert_eq!(u.poll().is_some(), asked);
            assert!(u.notice.is_none() && !u.checking());
        }
        let (tx, rx) = channel();
        let newer = Release {
            version: "999.0.0".into(),
            ..release
        };
        tx.send(Ok(update::compare(VERSION, newer))).unwrap();
        let mut u = Updates {
            job: Some((false, rx)),
            notice: None,
        };
        assert_eq!(u.poll(), None);
        assert_eq!(page(u.notice.as_ref().unwrap()), update::RELEASES_PAGE);
    }
}
