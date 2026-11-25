//! Applies the evening filter to the displays through the OS gamma tables.
//!
//! * Windows: `SetDeviceGammaRamp` on a device context per attached display. Windows rejects
//!   ramps that are too dark or too far from linear, so the channels are floored at
//!   [`MIN_CHANNEL`] (about 3400K with a little dimming) and, if a display still refuses,
//!   retried with weaker ramps. The approach (one ramp per display, identity ramp to restore)
//!   is the one LightBulb uses (https://github.com/Tyrrrz/LightBulb, MIT); the code is our own.
//! * macOS: `CGSetDisplayTransferByTable` per active display, `CGDisplayRestoreColorSyncSettings`
//!   to restore. macOS also drops these tables by itself when the process exits.
//! * Linux (X11): RandR `SetCrtcGamma` per CRTC through x11rb (pure Rust, no system library).
//!   Wayland has no portable gamma API, so the filter reports itself as unavailable there.
//!
//! None of this is called from tests; the maths lives in `evening.rs`.

use crate::evening::ramp_table;

/// Lowest per-channel multiplier we ask Windows for; below this it refuses the ramp.
pub const MIN_CHANNEL: f64 = if cfg!(windows) { 0.52 } else { 0.0 };

/// Ok when the filter can run here, otherwise a short code the UI turns into a sentence
/// ("wayland", "no-display", "unsupported").
pub fn support() -> Result<(), &'static str> {
    imp::support()
}

/// Sets the per-channel multipliers (0..=1) on every display.
pub fn apply(mult: [f32; 3]) -> Result<(), String> {
    imp::apply(mult)
}

/// Puts every display back to its normal, untouched look.
pub fn reset() -> Result<(), String> {
    imp::reset()
}

#[cfg(windows)]
mod imp {
    use super::ramp_table;
    use windows_sys::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, EnumDisplayDevicesW, DISPLAY_DEVICEW};
    use windows_sys::Win32::UI::ColorSystem::SetDeviceGammaRamp;

    const ATTACHED_TO_DESKTOP: u32 = 0x1;
    const MIRRORING_DRIVER: u32 = 0x8;

    pub fn support() -> Result<(), &'static str> {
        Ok(())
    }

    /// Device names of the displays currently part of the desktop (`\\.\DISPLAY1`, ...), as UTF-16.
    fn displays() -> Vec<[u16; 32]> {
        let mut out = Vec::new();
        for i in 0..32 {
            // SAFETY: DISPLAY_DEVICEW is plain data; `cb` is set as the API requires.
            let mut dev: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
            dev.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            let ok = unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dev, 0) };
            if ok == 0 {
                break;
            }
            if dev.StateFlags & ATTACHED_TO_DESKTOP != 0 && dev.StateFlags & MIRRORING_DRIVER == 0 {
                out.push(dev.DeviceName);
            }
        }
        out
    }

    /// Writes one ramp (red, green, blue; 256 entries each) to the display `name`.
    fn set_ramp(name: &[u16; 32], ramp: &[u16]) -> bool {
        let driver: Vec<u16> = "DISPLAY\0".encode_utf16().collect();
        // SAFETY: `name` is a NUL-terminated device name from EnumDisplayDevicesW and `ramp`
        // holds the 3 x 256 u16 values SetDeviceGammaRamp reads; the DC is deleted again.
        unsafe {
            let dc = CreateDCW(driver.as_ptr(), name.as_ptr(), std::ptr::null(), std::ptr::null());
            if dc.is_null() {
                return false;
            }
            let ok = SetDeviceGammaRamp(dc, ramp.as_ptr() as *const _);
            DeleteDC(dc);
            ok != 0
        }
    }

    fn ramp_for(m: [f32; 3]) -> Vec<u16> {
        let mut ramp = Vec::with_capacity(768);
        for c in m {
            ramp.extend(ramp_table(c, 256));
        }
        ramp
    }

    pub fn apply(mult: [f32; 3]) -> Result<(), String> {
        let displays = displays();
        if displays.is_empty() {
            return Err("no display found".into());
        }
        let mut accepted = 0;
        for d in &displays {
            // If the driver refuses the ramp, retry with progressively milder ones.
            let ok = [0.0f32, 0.7, 0.85]
                .iter()
                .any(|floor| set_ramp(d, &ramp_for(mult.map(|c| c.max(*floor)))));
            if ok {
                accepted += 1;
            }
        }
        if accepted == 0 {
            Err("Windows rejected the gamma ramp on every display".into())
        } else {
            Ok(())
        }
    }

    pub fn reset() -> Result<(), String> {
        let identity = ramp_for([1.0; 3]);
        let displays = displays();
        let failed = displays.iter().filter(|d| !set_ramp(d, &identity)).count();
        if failed > 0 {
            Err(format!("couldn't restore {failed} display(s)"))
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ramp_table;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        fn CGSetDisplayTransferByTable(display: u32, size: u32, red: *const f32, green: *const f32, blue: *const f32) -> i32;
        fn CGDisplayRestoreColorSyncSettings();
    }

    pub fn support() -> Result<(), &'static str> {
        Ok(())
    }

    fn active_displays() -> Vec<u32> {
        let mut ids = [0u32; 32];
        let mut n = 0u32;
        // SAFETY: `ids` has room for the 32 entries we pass as the maximum.
        let err = unsafe { CGGetActiveDisplayList(32, ids.as_mut_ptr(), &mut n) };
        if err != 0 {
            return Vec::new();
        }
        ids[..(n as usize).min(32)].to_vec()
    }

    pub fn apply(mult: [f32; 3]) -> Result<(), String> {
        let displays = active_displays();
        if displays.is_empty() {
            return Err("no display found".into());
        }
        let table = |m: f32| -> Vec<f32> { ramp_table(m, 256).iter().map(|v| *v as f32 / 65535.0).collect() };
        let (r, g, b) = (table(mult[0]), table(mult[1]), table(mult[2]));
        let mut ok = 0;
        for d in displays {
            // SAFETY: three 256-entry tables, as the size argument says.
            let err = unsafe { CGSetDisplayTransferByTable(d, 256, r.as_ptr(), g.as_ptr(), b.as_ptr()) };
            if err == 0 {
                ok += 1;
            }
        }
        if ok == 0 {
            Err("CoreGraphics rejected the gamma table".into())
        } else {
            Ok(())
        }
    }

    pub fn reset() -> Result<(), String> {
        // SAFETY: no arguments; restores every display to its ColorSync profile.
        unsafe { CGDisplayRestoreColorSyncSettings() };
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::ramp_table;
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;

    pub fn support() -> Result<(), &'static str> {
        // Wayland compositors don't expose gamma to ordinary apps (and XWayland's RandR
        // gamma does nothing), so don't pretend.
        if std::env::var("XDG_SESSION_TYPE").map_or(false, |s| s.eq_ignore_ascii_case("wayland")) {
            return Err("wayland");
        }
        if std::env::var_os("DISPLAY").is_none() {
            return Err("no-display");
        }
        Ok(())
    }

    fn set(mult: [f32; 3]) -> Result<(), String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen].root;
        let res = conn
            .randr_get_screen_resources_current(root)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let mut ok = 0;
        for &crtc in &res.crtcs {
            let size = match conn.randr_get_crtc_gamma_size(crtc).ok().and_then(|c| c.reply().ok()) {
                Some(r) if r.size > 1 => r.size as usize,
                _ => continue, // not driving a display
            };
            let (r, g, b) = (ramp_table(mult[0], size), ramp_table(mult[1], size), ramp_table(mult[2], size));
            if conn.randr_set_crtc_gamma(crtc, &r, &g, &b).ok().and_then(|c| c.check().ok()).is_some() {
                ok += 1;
            }
        }
        conn.flush().map_err(|e| e.to_string())?;
        if ok == 0 {
            Err("RandR gamma isn't available on this display".into())
        } else {
            Ok(())
        }
    }

    pub fn apply(mult: [f32; 3]) -> Result<(), String> {
        set(mult)
    }

    pub fn reset() -> Result<(), String> {
        set([1.0; 3])
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    pub fn support() -> Result<(), &'static str> {
        Err("unsupported")
    }
    pub fn apply(_: [f32; 3]) -> Result<(), String> {
        Err("unsupported".into())
    }
    pub fn reset() -> Result<(), String> {
        Ok(())
    }
}
