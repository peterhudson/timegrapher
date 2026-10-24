//! A USB microphone's automatic gain switch, set over USB itself.
//!
//! USB Audio Class 1 stands (the C-Media CM108/CM119 family among them)
//! carry the switch on a feature unit of the capture path, and turn it
//! back on each time they are plugged in. On Linux the ALSA mixer shows it
//! as "Auto Gain Control" (see [`crate::mixer`]); macOS shows no such
//! setting, so there it is read and set with the class request the ALSA
//! driver sends, on the device's control pipe (endpoint 0). The streaming
//! interface the system's audio driver holds is not touched.
//!
//! The descriptor parsing and the request bytes are plain functions,
//! tested on any system; only the transfer itself is macOS code.
//!
//! On Windows the system's USB audio driver offers the same switch to
//! programs as an endpoint's automatic gain control (the "AGC" box in the
//! Sound control panel), read and set by [`endpoint_agc`].

use serde::Serialize;

/// Feature unit control selector for automatic gain (USB Audio 1.0, A.10.2).
pub const AGC_CONTROL: u8 = 0x07;
/// Feature unit control selector for volume.
pub const VOLUME_CONTROL: u8 = 0x02;
/// Class requests (USB Audio 1.0, A.9).
pub const SET_CUR: u8 = 0x01;
pub const GET_CUR: u8 = 0x81;
pub const GET_MIN: u8 = 0x82;
pub const GET_MAX: u8 = 0x83;

/// The automatic gain switch of one feature unit on a capture path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AgcControl {
    /// The AudioControl interface's number.
    pub interface: u8,
    /// The feature unit's ID.
    pub unit: u8,
    /// The channel the switch is on: 0 for the master channel.
    pub channel: u8,
    /// Whether the same unit and channel have a volume control.
    pub volume: bool,
}

/// One class request on the control pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

/// A request to a feature unit's control: `request` is [`SET_CUR`] (host
/// to device) or one of the GETs (device to host).
pub fn feature_request(c: &AgcControl, request: u8, selector: u8, length: u16) -> Request {
    Request {
        // Class request to an interface; bit 7 set for device to host.
        request_type: if request & 0x80 != 0 { 0xA1 } else { 0x21 },
        request,
        value: ((selector as u16) << 8) | c.channel as u16,
        index: ((c.unit as u16) << 8) | c.interface as u16,
        length,
    }
}

/// The automatic gain switches on the capture paths of a configuration
/// descriptor (the whole of it, as GET_DESCRIPTOR returns it): each
/// feature unit with an AGC control between an input terminal and a USB
/// streaming output terminal, the one that carries sound to the computer.
pub fn capture_agc(config: &[u8]) -> Vec<AgcControl> {
    #[derive(Clone)]
    enum Entity {
        Input,
        Output { usb: bool, source: u8 },
        Unit { sources: Vec<u8> },
        Feature { source: u8, controls: Vec<u32> },
    }
    let mut entities: Vec<(u8, u8, Entity)> = Vec::new();
    let mut ac_interface: Option<u8> = None;
    let mut i = 0;
    while i + 2 <= config.len() {
        let len = config[i] as usize;
        if len < 2 || i + len > config.len() {
            break;
        }
        let d = &config[i..i + len];
        match d[1] {
            // An interface: AudioControl is class 1, subclass 1.
            0x04 if len >= 9 => {
                ac_interface = (d[5] == 1 && d[6] == 1).then_some(d[2]);
            }
            // Class-specific, inside an AudioControl interface.
            0x24 if len >= 4 => {
                if let Some(ifc) = ac_interface {
                    let id = d[3];
                    let e = match d[2] {
                        0x02 => Some(Entity::Input),
                        0x03 if len >= 8 => Some(Entity::Output {
                            usb: u16::from_le_bytes([d[4], d[5]]) == 0x0101,
                            source: d[7],
                        }),
                        0x04 | 0x05 if len >= 5 => {
                            let n = d[4] as usize;
                            Some(Entity::Unit {
                                sources: d.get(5..5 + n).unwrap_or(&[]).to_vec(),
                            })
                        }
                        0x06 if len >= 7 && d[5] > 0 => {
                            let size = d[5] as usize;
                            let controls = d[6..len - 1]
                                .chunks_exact(size)
                                .map(|c| {
                                    c.iter()
                                        .take(4)
                                        .enumerate()
                                        .fold(0u32, |a, (k, &b)| a | ((b as u32) << (8 * k)))
                                })
                                .collect();
                            Some(Entity::Feature {
                                source: d[4],
                                controls,
                            })
                        }
                        _ => None,
                    };
                    if let Some(e) = e {
                        entities.push((ifc, id, e));
                    }
                }
            }
            _ => {}
        }
        i += len;
    }
    let find = |ifc: u8, id: u8| {
        entities
            .iter()
            .find(|e| e.0 == ifc && e.1 == id && !matches!(e.2, Entity::Output { .. }))
    };
    let mut out = Vec::new();
    for (ifc, _, e) in &entities {
        let Entity::Output { usb: true, source } = e else {
            continue;
        };
        // Back from the output terminal to the input terminals.
        let mut stack = vec![*source];
        let mut seen = Vec::new();
        while let Some(id) = stack.pop() {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            match find(*ifc, id).map(|e| &e.2) {
                Some(Entity::Feature { source, controls }) => {
                    for (ch, &bits) in controls.iter().enumerate() {
                        // D6 is automatic gain, D1 volume.
                        if bits & (1 << 6) != 0 {
                            let c = AgcControl {
                                interface: *ifc,
                                unit: id,
                                channel: ch as u8,
                                volume: bits & (1 << 1) != 0,
                            };
                            if !out.contains(&c) {
                                out.push(c);
                            }
                        }
                    }
                    stack.push(*source);
                }
                Some(Entity::Unit { sources }) => stack.extend(sources),
                _ => {}
            }
        }
    }
    out
}

/// A volume control's value in dB: 1/256 dB steps, signed (USB Audio 1.0,
/// 5.2.2.4.3.2).
pub fn volume_db(raw: [u8; 2]) -> f64 {
    i16::from_le_bytes(raw) as f64 / 256.0
}

/// One USB microphone's automatic gain switch, as read and set.
#[derive(Debug, Clone, Serialize)]
pub struct UsbAgc {
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub control: AgcControl,
    /// The switch before any change, when it could be read.
    pub before: Option<bool>,
    /// After the change, read back.
    pub after: Option<bool>,
    /// The capture volume in dB: current, lowest and highest.
    pub volume_db: Option<(f64, f64, f64)>,
    /// What failed, when something did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Read every USB microphone's automatic gain switch, and with `set`
/// turn it on or off and read it back. macOS only: elsewhere the switch
/// is the ALSA mixer's (Linux) or the driver's.
pub fn usb_agc(set: Option<bool>) -> Result<Vec<UsbAgc>, String> {
    #[cfg(target_os = "macos")]
    {
        mac::usb_agc(set)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = set;
        Err(
            "setting a USB microphone's automatic gain over USB is for macOS; on Linux \
             it is the mixer's \"Auto Gain Control\" switch (timegrapher doctor)"
                .into(),
        )
    }
}

/// One capture endpoint's automatic gain switch, as read and set
/// (Windows).
#[derive(Debug, Clone, Serialize)]
pub struct EndpointAgc {
    /// The input's name, as the system shows it.
    pub name: String,
    /// The switch before any change, when it could be read.
    pub before: Option<bool>,
    /// After the change, read back.
    pub after: Option<bool>,
    /// What failed, when something did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Read the automatic gain switch of every active input with one, or of
/// those whose name contains `name`, and with `set` turn it on or off and
/// read it back. An input with no such switch is left out. Windows only:
/// elsewhere it is [`usb_agc`] (macOS) or the ALSA mixer (Linux).
pub fn endpoint_agc(name: Option<&str>, set: Option<bool>) -> Result<Vec<EndpointAgc>, String> {
    #[cfg(windows)]
    {
        win::endpoint_agc(name, set)
    }
    #[cfg(not(windows))]
    {
        let _ = (name, set);
        Err("an input's automatic gain control through the system is for Windows".into())
    }
}

#[cfg(windows)]
mod win {
    //! Through the endpoint's device topology: from the endpoint's
    //! connector back along the capture path, the first part that offers
    //! IAudioAutoGainControl.

    use super::EndpointAgc;
    use windows::core::Interface;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Media::Audio::{
        eCapture, IAudioAutoGainControl, IDeviceTopology, IMMDevice, IMMDeviceEnumerator, IPart,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::{
        PropVariantClear, PropVariantToStringAlloc,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED, STGM_READ,
    };

    pub fn endpoint_agc(name: Option<&str>, set: Option<bool>) -> Result<Vec<EndpointAgc>, String> {
        unsafe {
            // COM may already be set up on this thread (the app, cpal),
            // in either model; only what this call set up is undone.
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            let ours = hr.is_ok();
            if hr.is_err() && hr != RPC_E_CHANGED_MODE {
                return Err(format!("COM did not start ({hr})"));
            }
            let out = all(name, set);
            if ours {
                CoUninitialize();
            }
            out
        }
    }

    unsafe fn all(name: Option<&str>, set: Option<bool>) -> Result<Vec<EndpointAgc>, String> {
        let e = |x: windows::core::Error| x.to_string();
        let devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e)?;
        let list = devices
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .map_err(e)?;
        let mut out = Vec::new();
        for i in 0..list.GetCount().map_err(e)? {
            let Ok(device) = list.Item(i) else { continue };
            let label = friendly_name(&device).unwrap_or_else(|| "Unnamed input".into());
            if name.is_some_and(|n| !label.to_lowercase().contains(&n.to_lowercase())) {
                continue;
            }
            let Some(agc) = agc_of(&device) else { continue };
            let mut r = EndpointAgc {
                name: label,
                before: agc.GetEnabled().ok().map(|b| b.as_bool()),
                after: None,
                error: None,
            };
            if let Some(on) = set {
                match agc.SetEnabled(on, None) {
                    Ok(()) => r.after = agc.GetEnabled().ok().map(|b| b.as_bool()),
                    Err(x) => r.error = Some(x.to_string()),
                }
            }
            out.push(r);
        }
        Ok(out)
    }

    unsafe fn friendly_name(device: &IMMDevice) -> Option<String> {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut v = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let s = PropVariantToStringAlloc(&v).ok();
        let _ = PropVariantClear(&mut v);
        let s = s?;
        let text = s.to_string().ok();
        CoTaskMemFree(Some(s.0 as *const _));
        text
    }

    /// The automatic gain control on the endpoint's capture path, if any
    /// part along it offers one.
    unsafe fn agc_of(device: &IMMDevice) -> Option<IAudioAutoGainControl> {
        let topology: IDeviceTopology = device.Activate(CLSCTX_ALL, None).ok()?;
        let connector = topology.GetConnector(0).ok()?;
        let start: IPart = connector.GetConnectedTo().ok()?.cast().ok()?;
        // Upstream from the adapter's side of the endpoint, breadth first;
        // a topology is small, and the count bounds a looped one.
        let mut queue = vec![start];
        let mut seen = 0;
        while !queue.is_empty() && seen < 64 {
            let part = queue.remove(0);
            seen += 1;
            let mut p = std::ptr::null_mut();
            if part
                .Activate(CLSCTX_ALL.0, &IAudioAutoGainControl::IID, Some(&mut p))
                .is_ok()
                && !p.is_null()
            {
                return Some(IAudioAutoGainControl::from_raw(p));
            }
            if let Ok(incoming) = part.EnumPartsIncoming() {
                for k in 0..incoming.GetCount().unwrap_or(0) {
                    if let Ok(q) = incoming.GetPart(k) {
                        queue.push(q);
                    }
                }
            }
        }
        None
    }
}

#[cfg(target_os = "macos")]
mod mac {
    //! The transfer, through IOKit's USB device interface. The device is
    //! not opened when the request goes through without that, so the
    //! system's audio driver keeps it.

    use super::*;
    use std::ffi::{c_char, c_void, CStr};

    type KernReturn = i32;
    type IoObject = u32;
    type CFTypeRef = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CFUUIDBytes([u8; 16]);

    #[repr(C)]
    struct DevRequest {
        bm_request_type: u8,
        b_request: u8,
        w_value: u16,
        w_index: u16,
        w_length: u16,
        p_data: *mut c_void,
        w_len_done: u32,
    }

    /// The COM-style plug-in interface: IUnknown, then version fields.
    #[repr(C)]
    struct PlugIn {
        _reserved: *const c_void,
        query_interface:
            unsafe extern "C" fn(*mut *mut PlugIn, CFUUIDBytes, *mut *mut c_void) -> i32,
        _add_ref: *const c_void,
        release: unsafe extern "C" fn(*mut *mut PlugIn) -> u32,
    }

    /// IOUSBDeviceInterface (IOUSBLib.h), up to DeviceRequest.
    #[repr(C)]
    struct Device {
        _reserved: *const c_void,
        _query_interface: *const c_void,
        _add_ref: *const c_void,
        release: unsafe extern "C" fn(*mut *mut Device) -> u32,
        _create_async_event_source: *const c_void,
        _get_async_event_source: *const c_void,
        _create_async_port: *const c_void,
        _get_async_port: *const c_void,
        open: unsafe extern "C" fn(*mut *mut Device) -> KernReturn,
        close: unsafe extern "C" fn(*mut *mut Device) -> KernReturn,
        _class: *const c_void,
        _sub_class: *const c_void,
        _protocol: *const c_void,
        _vendor: *const c_void,
        _product: *const c_void,
        _release_number: *const c_void,
        _address: *const c_void,
        _bus_power: *const c_void,
        _speed: *const c_void,
        _configurations: *const c_void,
        _location: *const c_void,
        _configuration_descriptor: *const c_void,
        _get_configuration: *const c_void,
        _set_configuration: *const c_void,
        _bus_frame_number: *const c_void,
        _reset: *const c_void,
        device_request: unsafe extern "C" fn(*mut *mut Device, *mut DevRequest) -> KernReturn,
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingServices(
            port: u32,
            matching: *mut c_void,
            existing: *mut IoObject,
        ) -> KernReturn;
        fn IOIteratorNext(iterator: IoObject) -> IoObject;
        fn IOObjectRelease(object: IoObject) -> KernReturn;
        fn IORegistryEntryCreateCFProperty(
            entry: IoObject,
            key: CFTypeRef,
            allocator: CFTypeRef,
            options: u32,
        ) -> CFTypeRef;
        fn IOCreatePlugInInterfaceForService(
            service: IoObject,
            plugin_type: CFTypeRef,
            interface_type: CFTypeRef,
            the_interface: *mut *mut *mut PlugIn,
            score: *mut i32,
        ) -> KernReturn;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(a: CFTypeRef, s: *const c_char, enc: u32) -> CFTypeRef;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, enc: u32) -> u8;
        fn CFNumberGetValue(n: CFTypeRef, kind: isize, out: *mut c_void) -> u8;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFRelease(cf: CFTypeRef);
        fn CFUUIDGetConstantUUIDWithBytes(
            a: CFTypeRef,
            b0: u8,
            b1: u8,
            b2: u8,
            b3: u8,
            b4: u8,
            b5: u8,
            b6: u8,
            b7: u8,
            b8: u8,
            b9: u8,
            b10: u8,
            b11: u8,
            b12: u8,
            b13: u8,
            b14: u8,
            b15: u8,
        ) -> CFTypeRef;
    }

    const UTF8: u32 = 0x0800_0100;
    const SINT64: isize = 4;
    const NOT_OPEN: KernReturn = 0xe000_02cd_u32 as i32;

    const USB_DEVICE_USER_CLIENT: [u8; 16] = [
        0x9d, 0xc7, 0xb7, 0x80, 0x9e, 0xc0, 0x11, 0xd4, 0xa5, 0x4f, 0x00, 0x0a, 0x27, 0x05, 0x28,
        0x61,
    ];
    const CF_PLUGIN_INTERFACE: [u8; 16] = [
        0xc2, 0x44, 0xe8, 0x58, 0x10, 0x9c, 0x11, 0xd4, 0x91, 0xd4, 0x00, 0x50, 0xe4, 0xc6, 0x42,
        0x6f,
    ];
    const USB_DEVICE_INTERFACE: [u8; 16] = [
        0x5c, 0x81, 0x87, 0xd0, 0x9e, 0xf3, 0x11, 0xd4, 0x8b, 0x45, 0x00, 0x0a, 0x27, 0x05, 0x28,
        0x61,
    ];

    unsafe fn uuid(b: [u8; 16]) -> CFTypeRef {
        CFUUIDGetConstantUUIDWithBytes(
            std::ptr::null(),
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15],
        )
    }

    unsafe fn property(entry: IoObject, key: &CStr) -> CFTypeRef {
        let k = CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), UTF8);
        let v = IORegistryEntryCreateCFProperty(entry, k, std::ptr::null(), 0);
        CFRelease(k);
        v
    }

    unsafe fn number(entry: IoObject, key: &CStr) -> Option<i64> {
        let v = property(entry, key);
        if v.is_null() {
            return None;
        }
        let mut n: i64 = 0;
        let ok = CFGetTypeID(v) == CFNumberGetTypeID()
            && CFNumberGetValue(v, SINT64, &mut n as *mut i64 as *mut c_void) != 0;
        CFRelease(v);
        ok.then_some(n)
    }

    unsafe fn string(entry: IoObject, key: &CStr) -> Option<String> {
        let v = property(entry, key);
        if v.is_null() {
            return None;
        }
        let mut buf = [0 as c_char; 256];
        let ok = CFGetTypeID(v) == CFStringGetTypeID()
            && CFStringGetCString(v, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0;
        CFRelease(v);
        ok.then(|| CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }

    /// An open device interface, released when dropped.
    struct Dev {
        d: *mut *mut Device,
        opened: bool,
    }

    impl Drop for Dev {
        fn drop(&mut self) {
            unsafe {
                if self.opened {
                    ((**self.d).close)(self.d);
                }
                ((**self.d).release)(self.d);
            }
        }
    }

    impl Dev {
        unsafe fn new(service: IoObject) -> Result<Dev, String> {
            let mut plugin: *mut *mut PlugIn = std::ptr::null_mut();
            let mut score = 0;
            let kr = IOCreatePlugInInterfaceForService(
                service,
                uuid(USB_DEVICE_USER_CLIENT),
                uuid(CF_PLUGIN_INTERFACE),
                &mut plugin,
                &mut score,
            );
            if kr != 0 || plugin.is_null() {
                return Err(format!("no USB device interface (0x{:08x})", kr as u32));
            }
            let mut d: *mut c_void = std::ptr::null_mut();
            let hr =
                ((**plugin).query_interface)(plugin, CFUUIDBytes(USB_DEVICE_INTERFACE), &mut d);
            ((**plugin).release)(plugin);
            if hr != 0 || d.is_null() {
                return Err(format!("no USB device interface (0x{:08x})", hr as u32));
            }
            Ok(Dev {
                d: d as *mut *mut Device,
                opened: false,
            })
        }

        /// One control transfer. When the system refuses it on a device
        /// that is not open, the device is opened (shared, never seized)
        /// and the request sent again.
        unsafe fn request(&mut self, r: Request, data: &mut [u8]) -> Result<usize, String> {
            let mut req = DevRequest {
                bm_request_type: r.request_type,
                b_request: r.request,
                w_value: r.value,
                w_index: r.index,
                w_length: r.length.min(data.len() as u16),
                p_data: data.as_mut_ptr() as *mut c_void,
                w_len_done: 0,
            };
            let mut kr = ((**self.d).device_request)(self.d, &mut req);
            if kr == NOT_OPEN && !self.opened {
                let o = ((**self.d).open)(self.d);
                if o != 0 {
                    return Err(format!(
                        "the system refused the request on a device it holds (0x{:08x}); \
                         opening it was refused too (0x{:08x})",
                        kr as u32, o as u32
                    ));
                }
                self.opened = true;
                kr = ((**self.d).device_request)(self.d, &mut req);
            }
            if kr != 0 {
                return Err(format!("USB request failed (0x{:08x})", kr as u32));
            }
            Ok(req.w_len_done as usize)
        }

        unsafe fn configuration(&mut self) -> Result<Vec<u8>, String> {
            let get = |length| Request {
                request_type: 0x80,
                request: 0x06,
                value: 0x0200,
                index: 0,
                length,
            };
            let mut head = [0u8; 9];
            self.request(get(9), &mut head)?;
            let total = u16::from_le_bytes([head[2], head[3]]).max(9);
            let mut all = vec![0u8; total as usize];
            let n = self.request(get(total), &mut all)?;
            all.truncate(n);
            Ok(all)
        }

        unsafe fn agc(&mut self, c: &AgcControl) -> Result<bool, String> {
            let mut b = [0u8; 1];
            self.request(feature_request(c, GET_CUR, AGC_CONTROL, 1), &mut b)?;
            Ok(b[0] != 0)
        }

        unsafe fn volume(&mut self, c: &AgcControl, request: u8) -> Option<f64> {
            let mut b = [0u8; 2];
            let n = self
                .request(feature_request(c, request, VOLUME_CONTROL, 2), &mut b)
                .ok()?;
            (n == 2).then(|| volume_db(b))
        }
    }

    pub fn usb_agc(set: Option<bool>) -> Result<Vec<UsbAgc>, String> {
        let mut out = Vec::new();
        let mut seen: Vec<i64> = Vec::new();
        // IOUSBHostDevice on current macOS, IOUSBDevice before it.
        for class in [c"IOUSBHostDevice", c"IOUSBDevice"] {
            unsafe {
                let mut iter: IoObject = 0;
                if IOServiceGetMatchingServices(0, IOServiceMatching(class.as_ptr()), &mut iter)
                    != 0
                {
                    continue;
                }
                loop {
                    let s = IOIteratorNext(iter);
                    if s == 0 {
                        break;
                    }
                    let location = number(s, c"locationID").unwrap_or(-1);
                    if location >= 0 && seen.contains(&location) {
                        IOObjectRelease(s);
                        continue;
                    }
                    seen.push(location);
                    out.extend(one(s, set));
                    IOObjectRelease(s);
                }
                IOObjectRelease(iter);
            }
        }
        Ok(out)
    }

    /// The device's capture switches, read, set and read back. Devices
    /// with no USB Audio class are skipped before any request is sent.
    unsafe fn one(s: IoObject, set: Option<bool>) -> Vec<UsbAgc> {
        let vendor_id = number(s, c"idVendor").unwrap_or(0) as u16;
        let product_id = number(s, c"idProduct").unwrap_or(0) as u16;
        let name = string(s, c"USB Product Name")
            .or_else(|| string(s, c"kUSBProductString"))
            .unwrap_or_else(|| format!("USB device {vendor_id:04x}:{product_id:04x}"));
        let Ok(mut dev) = Dev::new(s) else {
            return Vec::new();
        };
        let config = match dev.configuration() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for control in capture_agc(&config) {
            let mut r = UsbAgc {
                name: name.clone(),
                vendor_id,
                product_id,
                control,
                before: None,
                after: None,
                volume_db: None,
                error: None,
            };
            match dev.agc(&control) {
                Ok(on) => r.before = Some(on),
                Err(e) => r.error = Some(e),
            }
            if let Some(on) = set {
                let mut b = [on as u8];
                match dev.request(feature_request(&control, SET_CUR, AGC_CONTROL, 1), &mut b) {
                    Ok(_) => match dev.agc(&control) {
                        Ok(v) => r.after = Some(v),
                        Err(e) => r.error = Some(e),
                    },
                    Err(e) => r.error = Some(e),
                }
            }
            if control.volume {
                r.volume_db = dev.volume(&control, GET_CUR).and_then(|cur| {
                    Some((
                        cur,
                        dev.volume(&control, GET_MIN)?,
                        dev.volume(&control, GET_MAX)?,
                    ))
                });
            }
            out.push(r);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CM108-like configuration: playback from USB to the speaker
    /// through a feature unit with mute and volume, and capture from the
    /// microphone through a feature unit with mute, volume and automatic
    /// gain, then a selector, to USB.
    fn cm108_like() -> Vec<u8> {
        let mut c = vec![
            9, 0x02, 0, 0, 3, 1, 0, 0x80, 50, // configuration (length set below)
            9, 0x04, 0, 0, 0, 1, 1, 0, 0, // interface 0: AudioControl
            10, 0x24, 0x01, 0x00, 0x01, 0, 0, 2, 1, 2, // header
            12, 0x24, 0x02, 1, 0x01, 0x01, 0, 2, 3, 0, 0, 0, // IT 1: USB streaming
            12, 0x24, 0x02, 2, 0x01, 0x02, 0, 1, 0, 0, 0, 0, // IT 2: microphone
            9, 0x24, 0x03, 6, 0x01, 0x03, 0, 9, 0, // OT 6: speaker, from FU 9
            9, 0x24, 0x03, 7, 0x01, 0x01, 0, 8, 0, // OT 7: USB streaming, from SU 8
            7, 0x24, 0x05, 8, 1, 10, 0, // SU 8, from FU 10
            10, 0x24, 0x06, 9, 1, 1, 0x01, 0x02, 0x02, 0, // FU 9 from IT 1
            9, 0x24, 0x06, 10, 2, 1, 0x43, 0x00, 0, // FU 10 from IT 2: mute, volume, AGC
            9, 0x04, 1, 0, 0, 1, 2, 0, 0, // interface 1: AudioStreaming
        ];
        let total = c.len() as u16;
        c[2..4].copy_from_slice(&total.to_le_bytes());
        c
    }

    #[test]
    fn the_capture_paths_switch_is_found() {
        assert_eq!(
            capture_agc(&cm108_like()),
            vec![AgcControl {
                interface: 0,
                unit: 10,
                channel: 0,
                volume: true,
            }]
        );
    }

    #[test]
    fn a_playback_switch_is_not_the_capture_one() {
        let mut c = cm108_like();
        // Give the playback feature unit (FU 9) automatic gain too.
        let fu9 = c.windows(4).position(|w| w == [10, 0x24, 0x06, 9]).unwrap();
        c[fu9 + 6] |= 0x40;
        let found = capture_agc(&c);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].unit, 10);
    }

    #[test]
    fn a_device_with_no_audio_control_has_none() {
        let c = [
            9, 0x02, 18, 0, 1, 1, 0, 0x80, 50, 9, 0x04, 0, 0, 1, 3, 0, 0, 0,
        ];
        assert!(capture_agc(&c).is_empty());
        assert!(capture_agc(&[]).is_empty());
        // A truncated descriptor stops the walk rather than panicking.
        assert!(capture_agc(&cm108_like()[..40]).is_empty());
    }

    #[test]
    fn the_requests_are_the_ones_alsa_sends() {
        let c = AgcControl {
            interface: 0,
            unit: 10,
            channel: 0,
            volume: true,
        };
        assert_eq!(
            feature_request(&c, SET_CUR, AGC_CONTROL, 1),
            Request {
                request_type: 0x21,
                request: 0x01,
                value: 0x0700,
                index: 0x0a00,
                length: 1,
            }
        );
        assert_eq!(
            feature_request(&c, GET_CUR, AGC_CONTROL, 1),
            Request {
                request_type: 0xA1,
                request: 0x81,
                value: 0x0700,
                index: 0x0a00,
                length: 1,
            }
        );
        let ch1 = AgcControl { channel: 1, ..c };
        assert_eq!(
            feature_request(&ch1, GET_MAX, VOLUME_CONTROL, 2).value,
            0x0201
        );
    }

    #[test]
    fn volume_is_in_steps_of_a_256th_of_a_db() {
        assert_eq!(volume_db([0x00, 0x01]), 1.0);
        assert_eq!(volume_db([0x00, 0xf0]), -16.0);
        assert_eq!(volume_db([0x80, 0x00]), 0.5);
    }
}
