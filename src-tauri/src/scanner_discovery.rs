//! Explicit, idle-only discovery of the supported Minolta scanner through the
//! already installed SANE library. No scan or device-open operation is used.

use serde::Serialize;
use std::ffi::CStr;

const SANE_LIBRARY: &CStr = c"/opt/homebrew/opt/sane-backends/lib/libsane.1.dylib";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScannerDevice {
    pub id: String,
    pub display_name: String,
    pub vendor: String,
    pub model: String,
    pub device_type: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiscoveryState {
    NoScanner,
    OneScanner,
    MultipleScanners,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveryResult {
    pub state: DiscoveryState,
    pub devices: Vec<ScannerDevice>,
}

pub(crate) fn classify_sane_devices(devices: Vec<ScannerDevice>) -> DiscoveryResult {
    let devices = devices
        .into_iter()
        .filter(is_supported_minolta)
        .collect::<Vec<_>>();
    let state = match devices.len() {
        0 => DiscoveryState::NoScanner,
        1 => DiscoveryState::OneScanner,
        _ => DiscoveryState::MultipleScanners,
    };
    DiscoveryResult { state, devices }
}

fn is_supported_minolta(device: &ScannerDevice) -> bool {
    let identity = format!(
        "{} {} {} {}",
        device.id, device.vendor, device.model, device.device_type
    )
    .to_ascii_lowercase();
    identity.contains("minolta")
        && (identity.contains("dual ii")
            || identity.contains("dual-ii")
            || identity.contains("af-2820")
            || identity.contains("2820u"))
}

#[cfg(target_os = "macos")]
pub(crate) fn discover() -> Result<DiscoveryResult, String> {
    use std::{mem, ptr};

    type AuthCallback =
        Option<unsafe extern "C" fn(*const libc::c_char, *mut libc::c_char, *mut libc::c_char)>;
    type SaneInit = unsafe extern "C" fn(*mut libc::c_int, AuthCallback) -> libc::c_int;
    type SaneGetDevices =
        unsafe extern "C" fn(*mut *const *const SaneDevice, libc::c_int) -> libc::c_int;
    type SaneExit = unsafe extern "C" fn();

    #[repr(C)]
    struct SaneDevice {
        name: *const libc::c_char,
        vendor: *const libc::c_char,
        model: *const libc::c_char,
        device_type: *const libc::c_char,
    }

    struct Library(*mut libc::c_void);
    impl Drop for Library {
        fn drop(&mut self) {
            unsafe { libc::dlclose(self.0) };
        }
    }

    struct SaneSession(SaneExit);
    impl Drop for SaneSession {
        fn drop(&mut self) {
            unsafe { (self.0)() };
        }
    }

    unsafe fn symbol<T: Copy>(library: &Library, name: &'static [u8]) -> Result<T, String> {
        let pointer = libc::dlsym(library.0, name.as_ptr().cast());
        if pointer.is_null() {
            return Err(format!(
                "SANE-Symbol {} fehlt",
                String::from_utf8_lossy(&name[..name.len() - 1])
            ));
        }
        Ok(mem::transmute_copy(&pointer))
    }

    unsafe fn copy_string(value: *const libc::c_char) -> String {
        if value.is_null() {
            String::new()
        } else {
            CStr::from_ptr(value).to_string_lossy().into_owned()
        }
    }

    let raw_library =
        unsafe { libc::dlopen(SANE_LIBRARY.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if raw_library.is_null() {
        return Err("Installierte SANE-Bibliothek konnte nicht geladen werden".into());
    }
    let library = Library(raw_library);
    let sane_init: SaneInit = unsafe { symbol(&library, b"sane_init\0")? };
    let sane_get_devices: SaneGetDevices = unsafe { symbol(&library, b"sane_get_devices\0")? };
    let sane_exit: SaneExit = unsafe { symbol(&library, b"sane_exit\0")? };

    let mut version = 0;
    let status = unsafe { sane_init(&mut version, None) };
    if status != 0 {
        return Err(format!(
            "SANE-Initialisierung fehlgeschlagen (Status {status})"
        ));
    }
    let _session = SaneSession(sane_exit);

    let mut list: *const *const SaneDevice = ptr::null();
    let status = unsafe { sane_get_devices(&mut list, 1) };
    if status != 0 {
        return Err(format!(
            "SANE-Geräteerkennung fehlgeschlagen (Status {status})"
        ));
    }

    let mut devices = Vec::new();
    if !list.is_null() {
        let mut terminated = false;
        for index in 0..256 {
            let item = unsafe { *list.add(index) };
            if item.is_null() {
                terminated = true;
                break;
            }
            let device = unsafe { &*item };
            let id = unsafe { copy_string(device.name) };
            if id.is_empty() {
                continue;
            }
            let vendor = unsafe { copy_string(device.vendor) };
            let model = unsafe { copy_string(device.model) };
            let device_type = unsafe { copy_string(device.device_type) };
            let display_name = if vendor.is_empty() && model.is_empty() {
                "Minolta DiMAGE Scan Dual II".to_string()
            } else {
                format!("{} {}", vendor.trim(), model.trim())
                    .trim()
                    .to_string()
            };
            devices.push(ScannerDevice {
                id,
                display_name,
                vendor,
                model,
                device_type,
            });
        }
        if !terminated {
            return Err(
                "SANE-Geräteliste überschreitet die sichere Grenze von 256 Einträgen".into(),
            );
        }
    }
    Ok(classify_sane_devices(devices))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn discover() -> Result<DiscoveryResult, String> {
    Err("Scannererkennung ist ausschließlich auf macOS ARM64 verfügbar".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minolta(id: &str) -> ScannerDevice {
        ScannerDevice {
            id: id.into(),
            display_name: "Minolta DiMAGE Scan Dual II".into(),
            vendor: "Minolta".into(),
            model: "DiMAGE Scan Dual II".into(),
            device_type: "film scanner".into(),
        }
    }

    #[test]
    fn classifies_no_scanner_one_scanner_and_multiple_scanners() {
        assert_eq!(
            classify_sane_devices(vec![]).state,
            DiscoveryState::NoScanner
        );
        assert_eq!(
            classify_sane_devices(vec![minolta("avision:usb:001")]).state,
            DiscoveryState::OneScanner
        );
        let multiple =
            classify_sane_devices(vec![minolta("avision:usb:001"), minolta("avision:usb:002")]);
        assert_eq!(multiple.state, DiscoveryState::MultipleScanners);
        assert_eq!(multiple.devices.len(), 2);
    }

    #[test]
    fn excludes_non_minolta_and_non_dual_ii_sane_devices() {
        let mut other_brand = minolta("usb:other");
        other_brand.vendor = "Other".into();
        let mut other_minolta = minolta("usb:other-minolta");
        other_minolta.model = "DiMAGE Scan Elite".into();
        assert_eq!(
            classify_sane_devices(vec![other_brand, other_minolta]).state,
            DiscoveryState::NoScanner
        );
    }
}
