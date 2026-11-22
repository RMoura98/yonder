//! Small, dependency-light OS integrations: idle time, fullscreen / meeting
//! detection, screen locking, running hooks and taking screenshots.

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

// ---------------------------------------------------------------- idle time

/// Seconds since the last keyboard / mouse input.
#[cfg(windows)]
pub fn idle_secs() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info) == 0 {
            return 0;
        }
        let now = GetTickCount();
        (now.wrapping_sub(info.dwTime) / 1000) as u64
    }
}

#[cfg(target_os = "macos")]
pub fn idle_secs() -> u64 {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    // kCGEventSourceStateCombinedSessionState = 0, kCGAnyInputEventType = ~0
    let s = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    if s.is_finite() && s > 0.0 {
        s as u64
    } else {
        0
    }
}

#[cfg(target_os = "linux")]
pub fn idle_secs() -> u64 {
    // X11: xprintidle (ms). GNOME / Wayland: Mutter idle monitor over D-Bus.
    if let Ok(out) = quiet(&mut Command::new("xprintidle")).output() {
        if out.status.success() {
            if let Ok(ms) = String::from_utf8_lossy(&out.stdout).trim().parse::<u64>() {
                return ms / 1000;
            }
        }
    }
    if let Ok(out) = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Mutter.IdleMonitor",
            "--object-path",
            "/org/gnome/Mutter/IdleMonitor/Core",
            "--method",
            "org.gnome.Mutter.IdleMonitor.GetIdletime",
        ])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        let digits: String = s
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if let Ok(ms) = digits.parse::<u64>() {
            return ms / 1000;
        }
    }
    0
}

// ------------------------------------------------------- fullscreen detection

/// Apps whose fullscreen mode means "watching something" rather than "working".
const MEDIA_APPS: &[&str] = &[
    "chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "arc", "zen",
    "vlc", "mpv", "mpc-hc64", "mpc-hc", "potplayermini64", "wmplayer", "video.ui",
    "microsoft.media.player", "netflix", "plex", "kodi", "iina", "quicktime player", "infuse",
];

/// True when another app runs fullscreen (games, presentations, fullscreen video).
#[cfg(windows)]
pub fn fullscreen_app_active(scanner: &mut ProcessScanner) -> bool {
    use windows_sys::Win32::UI::Shell::SHQueryUserNotificationState;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut state = 0i32;
    let hr = unsafe { SHQueryUserNotificationState(&mut state as *mut _ as *mut _) };
    if hr != 0 {
        return false;
    }
    match state {
        // 3 = QUNS_RUNNING_D3D_FULL_SCREEN (games), 4 = QUNS_PRESENTATION_MODE
        3 | 4 => true,
        // 2 = QUNS_BUSY: some window covers the screen. Borderless terminals and
        // editors trigger this too, so only count it for browsers / video players.
        2 => {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
            scanner
                .name_of(pid)
                .map(|n| MEDIA_APPS.iter().any(|m| n == *m))
                .unwrap_or(false)
        }
        _ => false,
    }
}

#[cfg(not(windows))]
pub fn fullscreen_app_active(_scanner: &mut ProcessScanner) -> bool {
    let _ = MEDIA_APPS;
    false
}

// ---------------------------------------------------------- meeting detection

/// True while the microphone or camera is being used by any app.
#[cfg(windows)]
pub fn mic_or_camera_in_use() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    fn key_in_use(key: &RegKey) -> bool {
        let stop: u64 = key.get_value("LastUsedTimeStop").unwrap_or(1);
        let start: u64 = key.get_value("LastUsedTimeStart").unwrap_or(0);
        start > 0 && stop == 0
    }

    fn scan(key: &RegKey, depth: u8) -> bool {
        for name in key.enum_keys().flatten() {
            if let Ok(sub) = key.open_subkey(&name) {
                if key_in_use(&sub) {
                    return true;
                }
                if depth > 0 && name == "NonPackaged" && scan(&sub, depth - 1) {
                    return true;
                }
            }
        }
        false
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
    ["microphone", "webcam"].iter().any(|cap| {
        hkcu.open_subkey(format!(r"{base}\{cap}"))
            .map(|k| scan(&k, 1))
            .unwrap_or(false)
    })
}

#[cfg(target_os = "macos")]
pub fn mic_or_camera_in_use() -> bool {
    // Fallback heuristic: well-known meeting apps that are in an active call
    // spawn helper processes. Handled by the process scan in the engine.
    false
}

#[cfg(target_os = "linux")]
pub fn mic_or_camera_in_use() -> bool {
    // PulseAudio / PipeWire: any active recording stream counts as "in a call".
    if let Ok(out) = Command::new("pactl")
        .args(["list", "short", "source-outputs"])
        .output()
    {
        if out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty() {
            return true;
        }
    }
    false
}

/// Process names that only exist while a meeting is in progress.
pub const MEETING_PROCESS_HINTS: &[&str] = &["cpthost", "zoom.us.cpthost", "aomhost"];

// ------------------------------------------------------------- process scan

pub struct ProcessScanner {
    sys: sysinfo::System,
}

impl ProcessScanner {
    pub fn new() -> Self {
        Self {
            sys: sysinfo::System::new(),
        }
    }

    /// Lower-cased name (without .exe) of a single process.
    #[allow(dead_code)]
    pub fn name_of(&mut self, pid: u32) -> Option<String> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate};
        if pid == 0 {
            return None;
        }
        let pid = Pid::from_u32(pid);
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        self.sys.process(pid).map(|p| {
            let n = p.name().to_string_lossy().to_lowercase();
            n.trim_end_matches(".exe").to_string()
        })
    }

    /// Lower-cased names of running processes (without .exe).
    pub fn names(&mut self) -> Vec<String> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        self.sys
            .processes()
            .values()
            .map(|p| {
                let n = p.name().to_string_lossy().to_lowercase();
                n.trim_end_matches(".exe").to_string()
            })
            .collect()
    }
}

// ---------------------------------------------------------------- tray glyph

/// Rasterizes the Yonder tray glyph (low sun on a horizon, one reflection),
/// drawn on a 16-unit grid, as RGBA at `size` px with 4x4 supersampling.
pub fn tray_glyph(size: u32, rgb: [u8; 3], alpha: f32) -> Vec<u8> {
    fn rrect(u: f32, v: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
        if u < x0 || u > x1 || v < y0 || v > y1 {
            return false;
        }
        let cx = u.clamp(x0 + r, x1 - r);
        let cy = v.clamp(y0 + r, y1 - r);
        (u - cx).powi(2) + (v - cy).powi(2) <= r * r
    }
    let inside = |u: f32, v: f32| -> bool {
        let sun = v <= 9.0 && (u - 8.0).powi(2) + (v - 9.0).powi(2) <= 25.0;
        sun || rrect(u, v, 1.0, 10.0, 15.0, 12.0, 1.0) || rrect(u, v, 5.0, 13.5, 11.0, 15.0, 0.75)
    };
    const SS: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let scale = 16.0 / size as f32;
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (px as f32 + (sx as f32 + 0.5) / SS as f32) * scale;
                    let v = (py as f32 + (sy as f32 + 0.5) / SS as f32) * scale;
                    if inside(u, v) {
                        hits += 1;
                    }
                }
            }
            let a = (hits as f32 / (SS * SS) as f32 * alpha * 255.0).round() as u8;
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn tray_glyph_renders_horizon() {
        let size = 16;
        let px = super::tray_glyph(size, [255, 255, 255], 1.0);
        let mut art = String::new();
        for y in 0..size {
            for x in 0..size {
                let a = px[((y * size + x) * 4 + 3) as usize];
                art.push(match a { 0..=40 => '.', 41..=170 => '+', _ => '#' });
            }
            art.push('\n');
        }
        println!("{art}");
        // sun above the horizon, horizon wider than the reflection
        let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
        assert!(alpha(8, 6) > 200, "sun");
        assert!(alpha(1, 11) > 100 && alpha(14, 11) > 100, "horizon");
        assert!(alpha(2, 14) < 40 && alpha(8, 14) > 100, "reflection");
    }
}

/// Whether the Windows taskbar uses the light theme (so the glyph should be dark).
#[cfg(windows)]
pub fn taskbar_is_light() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
        .map(|v| v == 1)
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn taskbar_is_light() -> bool {
    false
}

// --------------------------------------------------------------- open a URL

/// Opens an https URL in the default browser.
pub fn open_url(url: &str) {
    if !url.starts_with("https://") || url.contains(['"', '\'', ' ', '\n', '&', '|', '^']) {
        return;
    }
    #[cfg(windows)]
    let _ = quiet(Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url])).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

// -------------------------------------------------------------- lock screen

pub fn lock_screen() {
    #[cfg(windows)]
    let _ = quiet(Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"])).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("pmset").arg("displaysleepnow").spawn();
    #[cfg(target_os = "linux")]
    {
        if Command::new("loginctl").arg("lock-session").spawn().is_err() {
            let _ = Command::new("xdg-screensaver").arg("lock").spawn();
        }
    }
}

// -------------------------------------------------------------------- hooks

/// Runs a user automation command in the background.
pub fn run_hook(cmdline: &str, event: &str, kind: &str, secs: u64) {
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return;
    }
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", cmdline]);
        quiet(&mut c);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", cmdline]);
        c
    };
    let _ = cmd
        .env("YONDER_EVENT", event)
        .env("YONDER_BREAK_KIND", kind)
        .env("YONDER_BREAK_SECS", secs.to_string())
        .spawn();
}

// -------------------------------------------------------------- screenshots

/// A JPEG data URL of the monitor whose top-left corner is closest to (x, y).
#[cfg(not(target_os = "linux"))]
pub fn screenshot_near(x: i32, y: i32) -> Option<String> {
    use base64::Engine;
    use image::{codecs::jpeg::JpegEncoder, DynamicImage};

    let monitors = xcap::Monitor::all().ok()?;
    let mon = monitors.into_iter().min_by_key(|m| {
        let mx = m.x().unwrap_or(0);
        let my = m.y().unwrap_or(0);
        (mx - x).abs() + (my - y).abs()
    })?;
    let img = mon.capture_image().ok()?;
    let rgb = DynamicImage::ImageRgba8(img).to_rgb8();
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, 82).encode_image(&rgb).ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf)
    ))
}

#[cfg(target_os = "linux")]
pub fn screenshot_near(_x: i32, _y: i32) -> Option<String> {
    None
}
