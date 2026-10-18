//! Keeping the computer from going to sleep on its own while the app
//! listens, so a take left overnight isn't cut short by the power
//! settings. The screen may still lock and switch off, and the computer
//! still sleeps when told to (a closed lid, Sleep from a menu); only
//! sleeping from idleness waits. On macOS the same request stops the
//! system slowing the app down while its window is hidden (App Nap).
//!
//! Held and let go on the listening thread ([`crate::pump`]), which lives
//! as long as the app: Windows ties the request to the thread that made
//! it.

use crate::applog;

#[cfg(any(target_os = "macos", target_os = "linux"))]
const REASON: &str = "Listening to a watch";

/// Asks the system to stay awake while held.
#[derive(Default)]
pub struct Awake {
    held: Option<sys::Held>,
    /// The system refused once; not asked again until listening stops.
    refused: bool,
}

impl Awake {
    /// Hold the computer awake while `want`, let it go otherwise.
    pub fn hold(&mut self, want: bool) {
        if !want {
            if let Some(h) = self.held.take() {
                sys::release(h);
                applog::info("the computer may sleep on its own again");
            }
            self.refused = false;
            return;
        }
        if self.held.is_some() || self.refused {
            return;
        }
        match sys::hold() {
            Ok(h) => {
                applog::info("keeping the computer awake while listening");
                self.held = Some(h);
            }
            Err(e) => {
                applog::warn(&format!("couldn't keep the computer awake: {e}"));
                self.refused = true;
            }
        }
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        self.hold(false);
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
