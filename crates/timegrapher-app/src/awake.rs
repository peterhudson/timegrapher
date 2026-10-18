//! Keeping the computer from going to sleep on its own while the app
//! listens, so a take left overnight isn't cut short by the power
//! settings. The screen may still lock and switch off, and the computer
//! still sleeps when told to (a closed lid, Sleep from a menu); only
//! sleeping from idleness waits. On macOS the same request stops the
//! system slowing the app down while its window is hidden (App Nap).
//!
//! Held and let go on a thread of its own that lives as long as the app
//! (Windows ties the request to the thread that made it), so a slow or
//! stuck system call never holds up the listening thread or quitting.

use crate::applog;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "macos", target_os = "linux"))]
const REASON: &str = "Listening to a watch";

/// Asks the system to stay awake while held.
pub struct Awake {
    want: Option<Sender<bool>>,
    /// Asked for last.
    wanted: bool,
    /// Why the system didn't keep the computer awake, when it didn't.
    fault: Arc<Mutex<Option<String>>>,
}

impl Default for Awake {
    fn default() -> Self {
        let fault = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel();
        let f = fault.clone();
        let want = match std::thread::Builder::new()
            .name("keep awake".into())
            .spawn(move || run(&rx, &f))
        {
            Ok(_) => Some(tx),
            Err(e) => {
                *lock(&fault) = Some(format!("its thread didn't start: {e}"));
                None
            }
        };
        Awake {
            want,
            wanted: false,
            fault,
        }
    }
}

impl Awake {
    /// Hold the computer awake while `want`, let it go otherwise. Returns
    /// at once; the system is asked on the thread.
    pub fn hold(&mut self, want: bool) {
        if want == self.wanted {
            return;
        }
        self.wanted = want;
        if let Some(tx) = &self.want {
            let _ = tx.send(want);
        }
    }

    /// Why the computer isn't being kept awake although asked, if so.
    pub fn fault(&self) -> Option<String> {
        if self.wanted {
            lock(&self.fault).clone()
        } else {
            None
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Hold or let go as asked, until the app goes. A refusal isn't retried
/// until listening stops and starts again.
fn run(want: &Receiver<bool>, fault: &Mutex<Option<String>>) {
    let mut held = None;
    while let Ok(w) = want.recv() {
        if w && held.is_none() {
            match sys::hold() {
                Ok(h) => {
                    applog::info("keeping the computer awake while listening");
                    held = Some(h);
                    *lock(fault) = None;
                }
                Err(e) => {
                    applog::warn(&format!("couldn't keep the computer awake: {e}"));
                    *lock(fault) = Some(e);
                }
            }
        } else if !w {
            if let Some(h) = held.take() {
                sys::release(h);
                applog::info("the computer may sleep on its own again");
            }
            *lock(fault) = None;
        }
    }
    if let Some(h) = held {
        sys::release(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fault_shows_only_while_asked() {
        // No thread, so the system is never asked and the refusal stays.
        let mut a = Awake {
            want: None,
            wanted: false,
            fault: Arc::new(Mutex::new(Some("refused".into()))),
        };
        assert_eq!(a.fault(), None);
        a.hold(true);
        assert_eq!(a.fault().as_deref(), Some("refused"));
        a.hold(false);
        assert_eq!(a.fault(), None);
    }
}

#[cfg(windows)]
mod sys {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    pub struct Held;

    pub fn hold() -> Result<Held, String> {
        // SAFETY: takes flags only and changes nothing but this thread's
        // power request.
        match unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) } {
            0 => Err("Windows refused the request".into()),
            _ => Ok(Held),
        }
    }

    pub fn release(_: Held) {
        // SAFETY: as above; ES_CONTINUOUS alone clears the request.
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use objc2::rc::Retained;
    use objc2::runtime::{NSObjectProtocol, ProtocolObject};
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    /// The activity token, ended when let go.
    pub struct Held(Retained<ProtocolObject<dyn NSObjectProtocol>>);

    pub fn hold() -> Result<Held, String> {
        // User-initiated work: no idle sleep and no App Nap, while the
        // display may still sleep.
        let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiated,
            &NSString::from_str(super::REASON),
        );
        Ok(Held(token))
    }

    pub fn release(h: Held) {
        // SAFETY: the token is the one beginActivity returned.
        unsafe { NSProcessInfo::processInfo().endActivity(&h.0) };
    }
}

#[cfg(target_os = "linux")]
mod sys {
    //! The desktop portal's Inhibit with only its suspend flag, never idle:
    //! idle would also keep the screen lit and unlocked all night.
    use std::collections::HashMap;
    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedObjectPath, Value};

    const PORTAL: &str = "org.freedesktop.portal.Desktop";
    /// Inhibit's flag for suspending.
    const SUSPEND: u32 = 4;

    /// The session bus and the request whose closing ends the inhibit.
    pub struct Held(Connection, OwnedObjectPath);

    pub fn hold() -> Result<Held, String> {
        let bus = Connection::session().map_err(|e| e.to_string())?;
        let options = HashMap::from([("reason", Value::from(super::REASON))]);
        let reply = bus
            .call_method(
                Some(PORTAL),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Inhibit"),
                "Inhibit",
                &("", SUSPEND, options),
            )
            .map_err(|e| e.to_string())?;
        let request: OwnedObjectPath = reply.body().deserialize().map_err(|e| e.to_string())?;
        Ok(Held(bus, request))
    }

    pub fn release(Held(bus, request): Held) {
        let _ = bus.call_method(
            Some(PORTAL),
            &request,
            Some("org.freedesktop.portal.Request"),
            "Close",
            &(),
        );
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod sys {
    pub struct Held;

    pub fn hold() -> Result<Held, String> {
        Err("not on this system".into())
    }

    pub fn release(_: Held) {}
}
