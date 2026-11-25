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

/// Parse `gdbus call` output such as `(uint64 1234,)` or `(uint32 1234,)`
/// into milliseconds.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_gdbus_millis(out: &str) -> Option<u64> {
    let digits: String = out
        .split_whitespace()
        .nth(1)?
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// True on a Wayland session. There XWayland only sees input sent to X
/// clients, so `xprintidle` would report a bogus "idle" time and must not be used.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_wayland_session(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    session_type.map_or(false, |t| t.eq_ignore_ascii_case("wayland"))
        || wayland_display.map_or(false, |d| !d.is_empty())
}

#[cfg(target_os = "linux")]
fn gdbus_idle_ms(dest: &str, path: &str, method: &str) -> Option<u64> {
    let out = Command::new("gdbus")
        .args(["call", "--session", "--dest", dest, "--object-path", path, "--method", method])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_gdbus_millis(&String::from_utf8_lossy(&out.stdout))
}

/// Idle backends, tried in order; the last one that worked is tried first.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq)]
enum IdleSource {
    XPrintIdle,
    /// GNOME / Mutter.
    Mutter,
    /// KDE Plasma (KWin) implements the freedesktop ScreenSaver interface.
    ScreenSaver,
}

#[cfg(target_os = "linux")]
fn idle_from(src: IdleSource) -> Option<u64> {
    match src {
        IdleSource::XPrintIdle => {
            let out = quiet(&mut Command::new("xprintidle")).output().ok()?;
            if !out.status.success() {
                return None;
            }
            String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().ok()
        }
        IdleSource::Mutter => gdbus_idle_ms(
            "org.gnome.Mutter.IdleMonitor",
            "/org/gnome/Mutter/IdleMonitor/Core",
            "org.gnome.Mutter.IdleMonitor.GetIdletime",
        ),
        IdleSource::ScreenSaver => gdbus_idle_ms(
            "org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.GetSessionIdleTime",
        ),
    }
}

#[cfg(target_os = "linux")]
pub fn idle_secs() -> u64 {
    use std::sync::atomic::{AtomicU8, Ordering};
    // 0 = nothing worked yet, otherwise 1 + index into `ALL`.
    static LAST_GOOD: AtomicU8 = AtomicU8::new(0);
    const ALL: [IdleSource; 3] = [IdleSource::XPrintIdle, IdleSource::Mutter, IdleSource::ScreenSaver];

    let wayland = is_wayland_session(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
    );
    // ext-idle-notify-v1 first on Wayland: it is event driven (no process spawned
    // per tick), covers KWin and the wlroots family, and is simply `None` on
    // GNOME (Mutter lacks it), which then falls through to the D-Bus sources.
    if wayland {
        if let Some(ms) = crate::wayland_idle::idle_ms() {
            return ms / 1000;
        }
    }
    let candidates: Vec<usize> = (0..ALL.len())
        .filter(|&i| !(wayland && ALL[i] == IdleSource::XPrintIdle))
        .collect();
    let last = LAST_GOOD.load(Ordering::Relaxed) as usize;
    let mut order = Vec::with_capacity(candidates.len());
    if last > 0 && candidates.contains(&(last - 1)) {
        order.push(last - 1);
    }
    order.extend(candidates.into_iter().filter(|&i| i + 1 != last));

    for i in order {
        if let Some(ms) = idle_from(ALL[i]) {
            LAST_GOOD.store((i + 1) as u8, Ordering::Relaxed);
            return ms / 1000;
        }
    }
    // Nothing available: never claim the user is away.
    0
}

#[cfg(test)]
mod idle_tests {
    use super::*;

    #[test]
    fn parses_gdbus_output() {
        assert_eq!(parse_gdbus_millis("(uint64 5230,)
"), Some(5230));
        assert_eq!(parse_gdbus_millis("(uint32 17,)"), Some(17));
        assert_eq!(parse_gdbus_millis("Error: no such method"), None);
        assert_eq!(parse_gdbus_millis(""), None);
    }

    #[test]
    fn detects_wayland() {
        assert!(is_wayland_session(Some("wayland"), None));
        assert!(is_wayland_session(None, Some("wayland-0")));
        assert!(!is_wayland_session(Some("x11"), None));
        assert!(!is_wayland_session(None, Some("")));
    }
}

// ------------------------------------------------------- Do Not Disturb / Focus
// Approach for Windows' WNF Focus Assist state is the same idea Workrave (GPL, ideas
// only, no code taken) uses; the implementation here is original.

/// WNF buffer from `WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED`: a little-endian
/// integer, non-zero when a Focus / Do not disturb profile is active.
#[cfg_attr(not(windows), allow(dead_code))]
/// SHQueryUserNotificationState values that mean Windows' automatic Focus rules
/// (busy, D3D full screen, presentation), which aren't Do not disturb.
#[cfg_attr(not(windows), allow(dead_code))]
fn shell_state_rules_out_dnd(st: i32) -> bool {
    matches!(st, 2 | 3 | 4)
}

fn parse_wnf_profile(buf: &[u8]) -> bool {
    buf.iter().take(4).any(|b| *b != 0)
}

/// `gsettings get org.gnome.desktop.notifications show-banners` -> DND when "false".
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_gsettings_banners(out: &str) -> bool {
    out.trim() == "false"
}

/// Output of `busctl --user get-property ... Inhibited` ("b true") or
/// `gdbus call ... Get ... Inhibited` ("(<true>,)").
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_inhibited(out: &str) -> bool {
    let o = out.trim();
    o == "b true" || (o.contains("true") && !o.contains("false"))
}

/// True when macOS `Assertions.json` shows an active Focus assertion.
/// Format: `{"data":[{"storeAssertionRecords":[ ... ]}]}`; any record means Focus is on.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_macos_assertions(json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    v.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter().any(|e| {
                e.get("storeAssertionRecords")
                    .and_then(|r| r.as_array())
                    .map(|r| !r.is_empty())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn dnd_raw() -> bool {
    // Focus Assist / Do not disturb: the shell publishes the active quiet-hours
    // profile through an undocumented WNF state.
    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryWnfStateData(
            state_name: *const u32,
            type_id: *const core::ffi::c_void,
            explicit_scope: *const core::ffi::c_void,
            change_stamp: *mut u32,
            buffer: *mut core::ffi::c_void,
            buffer_size: *mut u32,
        ) -> i32;
    }
    use windows_sys::Win32::UI::Shell::SHQueryUserNotificationState;
    let mut st = 0i32;
    let hr = unsafe { SHQueryUserNotificationState(&mut st as *mut _ as *mut _) };
    // Windows' automatic rules (playing a game, an app in full screen, presenting)
    // also switch Focus Assist on. Those situations belong to the separate
    // "Fullscreen apps" rule, so while the shell reports BUSY (2), D3D full screen (3)
    // or presentation mode (4), only an explicit quiet-time state counts as DND.
    // Nothing else from it means DND: 5 (QUNS_ACCEPTS_NOTIFICATIONS) is the normal
    // state, and 6 (QUNS_QUIET_TIME) is the first hour after a new account's first
    // sign-in. Only the WNF profile below says whether Do not disturb is on.
    if hr == 0 && shell_state_rules_out_dnd(st) {
        return false;
    }
    // WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED = 0x0D83063EA3BF1C75
    let name: [u32; 2] = [0xA3BF_1C75, 0x0D83_063E];
    let mut stamp = 0u32;
    let mut buf = [0u8; 8];
    let mut size = buf.len() as u32;
    let status = unsafe {
        NtQueryWnfStateData(
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            &mut stamp,
            buf.as_mut_ptr() as *mut _,
            &mut size,
        )
    };
    if status != 0 {
        crate::logger::debug_once("dnd-wnf", &format!("Do Not Disturb: WNF query failed (status {status:#x}), treating as off"));
    }
    status == 0 && parse_wnf_profile(&buf[..(size as usize).min(buf.len())])
}

#[cfg(target_os = "macos")]
fn dnd_raw() -> bool {
    // No public API for Focus. Recent macOS keeps assertions in this file, but it is
    // only readable with Full Disk Access; without it we report false.
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let path = std::path::Path::new(&home).join("Library/DoNotDisturb/DB/Assertions.json");
    match std::fs::read_to_string(path) {
        Ok(s) => parse_macos_assertions(&s),
        Err(e) => {
            crate::logger::debug_once("dnd-macos", &format!("Do Not Disturb: can't read Assertions.json ({e}), treating as off"));
            false
        }
    }
}

#[cfg(target_os = "linux")]
fn dnd_raw() -> bool {
    let run = |prog: &str, args: &[&str]| -> Option<String> {
        let out = Command::new(prog).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    // GNOME: "Do Not Disturb" turns banners off.
    if let Some(o) = run("gsettings", &["get", "org.gnome.desktop.notifications", "show-banners"]) {
        if parse_gsettings_banners(&o) {
            return true;
        }
    }
    // KDE and other servers implementing the freedesktop Inhibited property.
    let o = run(
        "busctl",
        &[
            "--user", "get-property", "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "Inhibited",
        ],
    )
    .or_else(|| {
        run(
            "gdbus",
            &[
                "call", "--session", "--dest", "org.freedesktop.Notifications",
                "--object-path", "/org/freedesktop/Notifications", "--method",
                "org.freedesktop.DBus.Properties.Get", "org.freedesktop.Notifications", "Inhibited",
            ],
        )
    });
    if o.is_none() {
        crate::logger::debug_once("dnd-linux", "Do Not Disturb: neither busctl nor gdbus answered, treating as off");
    }
    o.map(|s| parse_inhibited(&s)).unwrap_or(false)
}

/// True while the system's Do Not Disturb / Focus mode is on. Cached for a few
/// seconds because it may spawn helper processes; fails silently to false.
pub fn dnd_active() -> bool {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, v)) = *c {
        if t.elapsed() < Duration::from_secs(5) {
            return v;
        }
    }
    let v = dnd_raw();
    *c = Some((Instant::now(), v));
    v
}

// ------------------------------------------------------- fullscreen detection

/// Apps whose fullscreen mode means "watching something" rather than "working".
#[cfg_attr(not(windows), allow(dead_code))]
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
    // Borderless "fullscreen windowed" games don't report D3D full screen; catch them
    // when the foreground app is a game Windows itself recognised (Game Bar's list)
    // and its window covers the whole monitor.
    {
        let hwnd = unsafe { GetForegroundWindow() };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if covers_monitor(hwnd) {
            if let Some(exe) = scanner.exe_of(pid) {
                if is_known_game(&exe, &known_games()) {
                    return true;
                }
            }
        }
    }
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

/// True when the window spans its whole monitor (borderless fullscreen).
#[cfg(windows)]
fn covers_monitor(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    if hwnd.is_null() {
        return false;
    }
    let mut r: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut r) } == 0 {
        return false;
    }
    let mon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi: MONITORINFO = unsafe { std::mem::zeroed() };
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(mon, &mut mi) } == 0 {
        return false;
    }
    let m = mi.rcMonitor;
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

/// Executables Windows recognised as games (Game Bar's GameConfigStore), lower-cased
/// full paths. Read from the registry at most once a minute.
#[cfg(windows)]
fn known_games() -> std::collections::HashSet<String> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, std::collections::HashSet<String>)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, set)) = c.as_ref() {
        if t.elapsed() < Duration::from_secs(60) {
            return set.clone();
        }
    }
    let mut set = std::collections::HashSet::new();
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    if let Ok(children) = hkcu.open_subkey(r"System\GameConfigStore\Children") {
        for name in children.enum_keys().flatten() {
            if let Ok(k) = children.open_subkey(&name) {
                if let Ok(p) = k.get_value::<String, _>("MatchedExeFullPath") {
                    set.insert(p.to_lowercase());
                }
            }
        }
    }
    *c = Some((Instant::now(), set.clone()));
    set
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_known_game(exe: &str, games: &std::collections::HashSet<String>) -> bool {
    !exe.is_empty() && games.contains(&exe.to_lowercase())
}

/// macOS: the frontmost normal (layer 0) window that isn't ours covers a whole display.
/// Real fullscreen hides the menu bar, so the window spans the full display rect. Only
/// window bounds / owner / layer are read, which needs no Screen Recording permission.
#[cfg(target_os = "macos")]
pub fn fullscreen_app_active(_scanner: &mut ProcessScanner) -> bool {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::display::CGDisplay;
    use core_graphics::geometry::CGRect;
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowBounds, kCGWindowLayer,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowOwnerPID,
    };

    const TOLERANCE: f64 = 2.0;
    const MIN_SIZE: f64 = 100.0;

    let displays: Vec<CGRect> = match CGDisplay::active_displays() {
        Ok(ids) => ids.into_iter().map(|id| CGDisplay::new(id).bounds()).collect(),
        Err(_) => return false,
    };
    if displays.is_empty() {
        return false;
    }
    let Some(windows) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return false;
    };

    // The keys are CoreGraphics-owned constants; wrap with the get rule (retain + release on drop).
    let (key_layer, key_pid, key_bounds) = unsafe {
        (
            CFString::wrap_under_get_rule(kCGWindowLayer),
            CFString::wrap_under_get_rule(kCGWindowOwnerPID),
            CFString::wrap_under_get_rule(kCGWindowBounds),
        )
    };
    let own_pid = std::process::id() as i64;

    // The list is ordered front to back; the first eligible layer-0 window decides.
    for item in windows.iter() {
        let ptr = *item;
        if ptr.is_null() {
            continue;
        }
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(ptr as CFDictionaryRef) };

        let number = |key: &CFString| -> Option<i64> {
            dict.find(key)?.downcast::<CFNumber>()?.to_i64()
        };
        if number(&key_layer) != Some(0) {
            continue;
        }
        if number(&key_pid) == Some(own_pid) {
            continue;
        }
        let Some(rect) = dict
            .find(&key_bounds)
            .and_then(|v| v.downcast::<CFDictionary>())
            .and_then(|d| CGRect::from_dict_representation(&d))
        else {
            continue;
        };
        // Tiny helper windows (status items, invisible panels) don't count.
        if rect.size.width < MIN_SIZE || rect.size.height < MIN_SIZE {
            continue;
        }
        return displays.iter().any(|d| {
            (rect.origin.x - d.origin.x).abs() <= TOLERANCE
                && (rect.origin.y - d.origin.y).abs() <= TOLERANCE
                && (rect.size.width - d.size.width).abs() <= TOLERANCE
                && (rect.size.height - d.size.height).abs() <= TOLERANCE
        });
    }
    false
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn fullscreen_app_active(_scanner: &mut ProcessScanner) -> bool {
    false
}

// ---------------------------------------------------------- meeting detection

/// True while the microphone or camera is being used by any app.
#[cfg(windows)]
pub fn mic_or_camera_in_use() -> bool {
    mic_or_camera_user().is_some()
}

/// The app using the microphone or camera, when the system says which one.
#[cfg(windows)]
pub fn mic_or_camera_app() -> Option<String> {
    mic_or_camera_user().filter(|n| !n.is_empty())
}

#[cfg(not(windows))]
pub fn mic_or_camera_app() -> Option<String> {
    None
}

/// Windows records mic/camera use per app in the privacy consent store; an entry
/// with a start time and no stop time is in use right now. Returns its app name
/// (empty when it can't be told).
#[cfg(windows)]
fn mic_or_camera_user() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    fn key_in_use(key: &RegKey) -> bool {
        let stop: u64 = key.get_value("LastUsedTimeStop").unwrap_or(1);
        let start: u64 = key.get_value("LastUsedTimeStart").unwrap_or(0);
        start > 0 && stop == 0
    }

    fn scan(key: &RegKey, depth: u8) -> Option<String> {
        for name in key.enum_keys().flatten() {
            if let Ok(sub) = key.open_subkey(&name) {
                if key_in_use(&sub) {
                    return Some(consent_app_name(&name));
                }
                if depth > 0 && name == "NonPackaged" {
                    if let Some(n) = scan(&sub, depth - 1) {
                        return Some(n);
                    }
                }
            }
        }
        None
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
    ["microphone", "webcam"].iter().find_map(|cap| {
        hkcu.open_subkey(format!(r"{base}\{cap}")).ok().and_then(|k| scan(&k, 1))
    })
}

/// A readable app name from a consent-store key: a desktop app's path with `#` for
/// `\` ("C:#Users#me#AppData#Local#Discord#app-1.0#Discord.exe" → "Discord"), or a
/// Store app's package family ("Microsoft.SkypeApp_kzf8qxf38zg5c" → "SkypeApp").
#[cfg_attr(not(windows), allow(dead_code))]
fn consent_app_name(key: &str) -> String {
    if key.contains('#') {
        let file = key.rsplit('#').next().unwrap_or(key);
        let stem = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
        return stem.to_string();
    }
    let family = key.split('_').next().unwrap_or(key);
    family.rsplit('.').next().unwrap_or(family).to_string()
}

/// The name of the app whose window is in front ("Enshrouded"), for messages.
#[cfg(windows)]
pub fn foreground_app_name(scanner: &mut ProcessScanner) -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    let exe = scanner.exe_of(pid)?;
    let stem = std::path::Path::new(&exe).file_stem()?.to_string_lossy().into_owned();
    (!stem.is_empty()).then_some(stem)
}

#[cfg(not(windows))]
pub fn foreground_app_name(_scanner: &mut ProcessScanner) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
pub fn mic_or_camera_in_use() -> bool {
    mac_av::mic_in_use() || mac_av::camera_in_use()
}

/// CoreAudio / CoreMediaIO "is something using this device" queries.
///
/// These only read device properties; they never open a stream, so macOS does
/// not show a microphone or camera permission prompt and no entitlement is
/// needed. They never touch audio or video data. (Zoom's meeting helper check
/// in the process scan stays as an extra fallback.)
#[cfg(target_os = "macos")]
mod mac_av {
    use std::os::raw::c_void;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const fn fourcc(s: &[u8; 4]) -> u32 {
        ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | s[3] as u32
    }

    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const SCOPE_INPUT: u32 = fourcc(b"inpt");
    const SCOPE_OUTPUT: u32 = fourcc(b"outp");
    const DEVICES: u32 = fourcc(b"dev#"); // kAudioHardwarePropertyDevices / kCMIOHardwarePropertyDevices
    const STREAMS: u32 = fourcc(b"stm#"); // kAudioDevicePropertyStreams
    const RUNNING_SOMEWHERE: u32 = fourcc(b"gone"); // kAudioDevicePropertyDeviceIsRunningSomewhere (same value in CMIO)
    const PROCESS_LIST: u32 = fourcc(b"prs#"); // kAudioHardwarePropertyProcessObjectList (macOS 14.2+)
    const PROCESS_RUNNING_INPUT: u32 = fourcc(b"piri"); // kAudioProcessPropertyIsRunningInput (macOS 14.2+)

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(id: u32, addr: *const Address, qsize: u32, q: *const c_void, size: *mut u32) -> i32;
        fn AudioObjectGetPropertyData(id: u32, addr: *const Address, qsize: u32, q: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    }

    #[link(name = "CoreMediaIO", kind = "framework")]
    extern "C" {
        fn CMIOObjectGetPropertyDataSize(id: u32, addr: *const Address, qsize: u32, q: *const c_void, size: *mut u32) -> i32;
        fn CMIOObjectGetPropertyData(id: u32, addr: *const Address, qsize: u32, q: *const c_void, size: u32, used: *mut u32, data: *mut c_void) -> i32;
    }

    type SizeFn = unsafe fn(u32, &Address) -> Option<u32>;
    type DataFn = unsafe fn(u32, &Address, u32, *mut c_void) -> bool;

    unsafe fn audio_size(id: u32, a: &Address) -> Option<u32> {
        let mut n = 0u32;
        (AudioObjectGetPropertyDataSize(id, a, 0, std::ptr::null(), &mut n) == 0).then_some(n)
    }
    unsafe fn audio_data(id: u32, a: &Address, size: u32, out: *mut c_void) -> bool {
        let mut n = size;
        AudioObjectGetPropertyData(id, a, 0, std::ptr::null(), &mut n, out) == 0
    }
    unsafe fn cmio_size(id: u32, a: &Address) -> Option<u32> {
        let mut n = 0u32;
        (CMIOObjectGetPropertyDataSize(id, a, 0, std::ptr::null(), &mut n) == 0).then_some(n)
    }
    unsafe fn cmio_data(id: u32, a: &Address, size: u32, out: *mut c_void) -> bool {
        let mut used = 0u32;
        CMIOObjectGetPropertyData(id, a, 0, std::ptr::null(), size, &mut used, out) == 0
    }

    fn addr(selector: u32, scope: u32) -> Address {
        Address { selector, scope, element: 0 }
    }

    unsafe fn ids(size_fn: SizeFn, data_fn: DataFn, object: u32, a: &Address) -> Option<Vec<u32>> {
        let bytes = size_fn(object, a)?;
        let mut v = vec![0u32; bytes as usize / 4];
        if v.is_empty() {
            return Some(v);
        }
        data_fn(object, a, bytes, v.as_mut_ptr() as *mut c_void).then_some(v)
    }

    unsafe fn flag(data_fn: DataFn, object: u32, a: &Address) -> bool {
        let mut v = 0u32;
        data_fn(object, a, 4, &mut v as *mut u32 as *mut c_void) && v != 0
    }

    /// True while any app is capturing audio input.
    pub fn mic_in_use() -> bool {
        unsafe {
            // macOS 14.2+: ask per client process whether it is running input.
            // Precise: a headset that is only playing audio does not count.
            if let Some(procs) = ids(audio_size, audio_data, SYSTEM_OBJECT, &addr(PROCESS_LIST, SCOPE_GLOBAL)) {
                let a = addr(PROCESS_RUNNING_INPUT, SCOPE_GLOBAL);
                return procs.iter().any(|p| flag(audio_data, *p, &a));
            }
            // Older macOS: input-only devices (built-in/USB mics). Duplex devices
            // (headsets) are skipped because "running" is also true while they
            // merely play audio.
            let Some(devs) = ids(audio_size, audio_data, SYSTEM_OBJECT, &addr(DEVICES, SCOPE_GLOBAL)) else {
                return false;
            };
            let running = addr(RUNNING_SOMEWHERE, SCOPE_GLOBAL);
            devs.iter().any(|d| {
                let has = |scope| audio_size(*d, &addr(STREAMS, scope)).unwrap_or(0) > 0;
                has(SCOPE_INPUT) && !has(SCOPE_OUTPUT) && flag(audio_data, *d, &running)
            })
        }
    }

    /// True while any app is capturing from a camera.
    pub fn camera_in_use() -> bool {
        unsafe {
            let Some(devs) = ids(cmio_size, cmio_data, SYSTEM_OBJECT, &addr(DEVICES, SCOPE_GLOBAL)) else {
                return false;
            };
            let running = addr(RUNNING_SOMEWHERE, SCOPE_GLOBAL);
            devs.iter().any(|d| flag(cmio_data, *d, &running))
        }
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetActiveDisplayList(max: u32, ids: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayIsInMirrorSet(id: u32) -> u32;
    }

    /// True while displays are mirrored (presenting on a projector or TV).
    /// Idea from EyeBreak's quiet mode (MIT, github.com/sviatil0/eyebreak).
    pub fn display_mirrored() -> bool {
        let mut ids = [0u32; 16];
        let mut n = 0u32;
        unsafe {
            CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0
                && ids[..(n as usize).min(16)].iter().any(|d| CGDisplayIsInMirrorSet(*d) != 0)
        }
    }
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

// ------------------------------------------------- screen share / recording

/// Recorders and streamers that normally run only while in use, matched by
/// lower-cased process name. Nothing can tell which app is capturing the
/// screen (Windows Graphics Capture and macOS ScreenCaptureKit expose no such
/// query), so this is a curated list: an app counts while it is open, not
/// strictly while it records. Always-on tray apps (Loom, ShareX, CleanShot)
/// are left out on purpose.
const SCREEN_CAPTURE_APPS: &[&str] = &[
    "obs64", "obs32", "obs", "streamlabs desktop", "streamlabs obs", "slobs",
    "xsplit.core", "xsplit.broadcaster", "camrecorder", "camtasia", "screenflow",
    "simplescreenrecorder", "kooha", "vokoscreenng", "wf-recorder", "peek", "bdcam",
];

/// First running process that is a known screen recorder, if any.
pub fn screen_capture_app(names: &[String]) -> Option<&'static str> {
    SCREEN_CAPTURE_APPS
        .iter()
        .copied()
        .find(|h| names.iter().any(|n| n == h || (h.len() >= 8 && n.starts_with(*h))))
}

/// True if a `pw-dump` JSON document has a PipeWire stream reading video that
/// looks like a screencast (the portal tags it with a "screen" role or name).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn pipewire_screencast(dump: &str) -> bool {
    let Ok(serde_json::Value::Array(objs)) = serde_json::from_str::<serde_json::Value>(dump) else {
        return false;
    };
    objs.iter().any(|o| {
        let props = &o["info"]["props"];
        props["media.class"] == "Stream/Input/Video"
            && ["media.role", "media.name", "node.name"]
                .iter()
                .any(|k| props[*k].as_str().is_some_and(|s| s.to_lowercase().contains("screen")))
    })
}

/// True while displays are mirrored (macOS, Windows) or a PipeWire screencast
/// is active (Linux). Recorder processes are matched by the caller.
pub fn screen_shared_by_system() -> bool {
    #[cfg(target_os = "macos")]
    {
        mac_av::display_mirrored()
    }
    #[cfg(windows)]
    {
        win_display_mirrored()
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("pw-dump")
            .output()
            .map(|o| o.status.success() && pipewire_screencast(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or(false)
    }
}

/// Duplicate display mode: two active paths share one source.
#[cfg(windows)]
fn win_display_mirrored() -> bool {
    use windows_sys::Win32::Devices::Display::*;
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != 0 || np < 2 {
            return false;
        }
        let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![std::mem::zeroed(); np as usize];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![std::mem::zeroed(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), std::ptr::null_mut()) != 0 {
            return false;
        }
        let src: Vec<(i32, u32, u32)> = paths[..np as usize]
            .iter()
            .map(|p| (p.sourceInfo.adapterId.HighPart, p.sourceInfo.adapterId.LowPart, p.sourceInfo.id))
            .collect();
        src.iter().enumerate().any(|(i, a)| src[i + 1..].contains(a))
    }
}

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

    /// Full path of a process's executable.
    #[allow(dead_code)]
    pub fn exe_of(&mut self, pid: u32) -> Option<String> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
        if pid == 0 {
            return None;
        }
        let pid = Pid::from_u32(pid);
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        self.sys
            .process(pid)
            .and_then(|p| p.exe())
            .map(|e| e.to_string_lossy().into_owned())
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

/// The countdown as shown in the tray icon: minutes ("12"), hours from 100 minutes ("2h"),
/// and plain seconds in the last minute (flagged `true`, drawn with an underline).
pub fn countdown_label(secs: i64) -> (String, bool) {
    let s = secs.max(0);
    if s < 60 {
        (s.to_string(), true)
    } else if s <= 99 * 60 {
        (((s + 59) / 60).to_string(), false)
    } else {
        (format!("{}h", ((s + 3599) / 3600).min(99)), false)
    }
}

/// 3x5 bitmap glyphs (rows top to bottom, bit 2 = left column) for the digits and "h".
fn digit_rows(c: char) -> [u8; 5] {
    match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'h' => [0b100, 0b100, 0b110, 0b101, 0b101],
        _ => [0; 5],
    }
}

/// Renders `text` (1-3 chars from `countdown_label`) as big blocky glyphs filling a `size` px
/// square icon (RGBA, solid `rgb`). `seconds` adds a bar beneath, marking a seconds value.
pub fn countdown_glyph(size: u32, text: &str, seconds: bool, rgb: [u8; 3]) -> Vec<u8> {
    let chars: Vec<char> = text.chars().take(3).collect();
    let n = chars.len().max(1) as u32;
    // Grid in cells: 3 wide per glyph, 1 gap between; 5 tall, plus a gap and a bar row for seconds.
    let (cols, rows) = (4 * n - 1, if seconds { 7 } else { 5 });
    let scale = (size / cols).min(size / rows).max(1);
    let (w, h) = (cols * scale, rows * scale);
    let (x0, y0) = (size.saturating_sub(w) / 2, size.saturating_sub(h) / 2);
    let mut out = vec![0u8; (size * size * 4) as usize];
    let mut put = |cx: u32, cy: u32| {
        for dy in 0..scale {
            for dx in 0..scale {
                let (x, y) = (x0 + cx * scale + dx, y0 + cy * scale + dy);
                if x < size && y < size {
                    let i = ((y * size + x) * 4) as usize;
                    out[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
        }
    };
    for (k, c) in chars.iter().enumerate() {
        for (r, bits) in digit_rows(*c).iter().enumerate() {
            for col in 0..3u32 {
                if (bits >> (2 - col)) & 1 == 1 {
                    put(k as u32 * 4 + col, r as u32);
                }
            }
        }
    }
    if seconds {
        for cx in 0..cols {
            put(cx, 6);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn countdown_labels() {
        use super::countdown_label as l;
        assert_eq!(l(12 * 60), ("12".into(), false));
        assert_eq!(l(12 * 60 - 1), ("12".into(), false));
        assert_eq!(l(60), ("1".into(), false));
        assert_eq!(l(59), ("59".into(), true));
        assert_eq!(l(0), ("0".into(), true));
        assert_eq!(l(-5), ("0".into(), true));
        assert_eq!(l(99 * 60), ("99".into(), false));
        assert_eq!(l(99 * 60 + 1), ("2h".into(), false));
        assert_eq!(l(100 * 3600), ("99h".into(), false));
    }

    #[test]
    fn countdown_glyph_draws_digits() {
        let size = 32;
        let a = |px: &[u8], x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
        for (text, secs) in [("8", false), ("12", false), ("45", true), ("2h", false), ("99", true)] {
            let px = super::countdown_glyph(size, text, secs, [1, 2, 3]);
            assert_eq!(px.len(), (size * size * 4) as usize);
            assert!(px.chunks(4).any(|p| p[3] == 255 && p[..3] == [1, 2, 3]), "{text} draws");
            // nothing touches the outer 1px border
            for i in 0..size {
                assert_eq!(a(&px, i, 0) | a(&px, 0, i) | a(&px, i, size - 1) | a(&px, size - 1, i), 0, "{text} margin");
            }
        }
        let ink = |t: &str, s: bool| super::countdown_glyph(size, t, s, [0; 3]).chunks(4).filter(|p| p[3] > 0).count();
        assert!(ink("8", false) > ink("1", false));
        assert!(ink("45", true) > ink("45", false));
        let _ = super::countdown_glyph(size, "", false, [0; 3]);
    }

    /// Writes sample icons when YONDER_ICON_DIR is set (manual eyeballing only).
    #[test]
    fn countdown_glyph_dump() {
        let Ok(dir) = std::env::var("YONDER_ICON_DIR") else { return };
        let mut sheet = vec![0u8; 32 * 6 * 32 * 2 * 4];
        for (i, (t, s)) in [("12", false), ("5", false), ("45", true), ("99", false), ("2h", false), ("0", true)].iter().enumerate() {
            for (row, rgb, bg) in [(0usize, [255u8, 255, 255], [32u8, 32, 32]), (1, [0, 0, 0], [243, 243, 243])] {
                let px = super::countdown_glyph(32, t, *s, rgb);
                for y in 0..32usize {
                    for x in 0..32usize {
                        let p = &px[(y * 32 + x) * 4..][..4];
                        let o = ((row * 32 + y) * 192 + i * 32 + x) * 4;
                        for c in 0..3 {
                            sheet[o + c] = if p[3] > 0 { p[c] } else { bg[c] };
                        }
                        sheet[o + 3] = 255;
                    }
                }
            }
        }
        let rgb: Vec<u8> = sheet.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        image::save_buffer(format!("{dir}/countdown.jpg"), &rgb, 192, 64, image::ColorType::Rgb8).unwrap();
    }

    #[test]
    fn dnd_parsers() {
        use super::*;
        assert!(parse_wnf_profile(&[1, 0, 0, 0]));
        assert!(!parse_wnf_profile(&[0, 0, 0, 0]));
        assert!(!parse_wnf_profile(&[]));
        assert!(parse_gsettings_banners("false\n"));
        assert!(!parse_gsettings_banners("true\n"));
        assert!(parse_inhibited("b true\n"));
        assert!(!parse_inhibited("b false\n"));
        assert!(parse_inhibited("(<true>,)"));
        assert!(!parse_inhibited("(<false>,)"));
        assert!(!parse_inhibited(""));
        assert!(parse_macos_assertions(r#"{"data":[{"storeAssertionRecords":[{"x":1}]}]}"#));
        assert!(!parse_macos_assertions(r#"{"data":[{"storeAssertionRecords":[]}]}"#));
        assert!(!parse_macos_assertions("garbage"));
        // Windows' automatic Focus rules aren't DND; the normal state (5) and quiet
        // time (6) leave the decision to the WNF profile.
        assert!(shell_state_rules_out_dnd(2) && shell_state_rules_out_dnd(3) && shell_state_rules_out_dnd(4));
        assert!(!shell_state_rules_out_dnd(5) && !shell_state_rules_out_dnd(6));
    }

    #[test]
    #[ignore]
    fn print_dnd_state() {
        println!("dnd_active = {}", super::dnd_active());
    }

    #[test]
    fn matches_screen_recorders() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(super::screen_capture_app(&n(&["chrome", "obs64"])), Some("obs64"));
        assert_eq!(super::screen_capture_app(&n(&["camtasia 2024"])), Some("camtasia"));
        assert_eq!(super::screen_capture_app(&n(&["chrome", "explorer", "obsidian"])), None);
        assert_eq!(super::screen_capture_app(&[]), None);
    }

    #[test]
    fn detects_pipewire_screencast() {
        let yes = r#"[{"info":{"props":{"media.class":"Stream/Input/Video","media.role":"Screen"}}}]"#;
        let cam = r#"[{"info":{"props":{"media.class":"Stream/Input/Video","media.role":"Camera"}}}]"#;
        assert!(super::pipewire_screencast(yes));
        assert!(!super::pipewire_screencast(cam));
        assert!(!super::pipewire_screencast("not json"));
    }

    #[test]
    fn consent_store_keys_become_app_names() {
        use super::consent_app_name;
        assert_eq!(consent_app_name(r"C:#Users#me#AppData#Local#Discord#app-1.0.9269#Discord.exe"), "Discord");
        assert_eq!(consent_app_name(r"C:#Program Files#Zoom#bin#Zoom.EXE"), "Zoom");
        assert_eq!(consent_app_name("Microsoft.SkypeApp_kzf8qxf38zg5c"), "SkypeApp");
        assert_eq!(consent_app_name("MSTeams_8wekyb3d8bbwe"), "MSTeams");
    }

    #[test]
    fn known_game_matches_case_insensitively() {
        let games: std::collections::HashSet<String> =
            [r"c:\games\enshrouded\enshrouded.exe".to_string()].into_iter().collect();
        assert!(super::is_known_game(r"C:\Games\Enshrouded\enshrouded.exe", &games));
        assert!(!super::is_known_game(r"C:\Windows\explorer.exe", &games));
        assert!(!super::is_known_game("", &games));
    }

    #[test]
    #[ignore]
    fn print_fullscreen_state() {
        let mut s = super::ProcessScanner::new();
        println!("fullscreen_app_active = {}", super::fullscreen_app_active(&mut s));
    }

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

/// The system's double-click time in milliseconds.
#[cfg(windows)]
pub fn double_click_ms() -> u64 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    let ms = unsafe { GetDoubleClickTime() } as u64;
    if ms == 0 { 500 } else { ms }
}

/// macOS sends no tray double clicks, so Yonder spots two clicks within this window itself.
/// Kept short: a single click waits this long before the popover opens.
#[cfg(target_os = "macos")]
pub fn double_click_ms() -> u64 {
    300
}

/// No tray double clicks here, so a single click needn't wait.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn double_click_ms() -> u64 {
    0
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

/// Opens a folder in the file manager.
pub fn open_folder(dir: &std::path::Path) {
    #[cfg(windows)]
    let _ = quiet(Command::new("explorer.exe").arg(dir)).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(dir).spawn();
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(dir).spawn();
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

// ------------------------------------------------------ app focus (macOS breaks)

/// Process id of the frontmost app.
#[cfg(target_os = "macos")]
pub fn frontmost_app_pid() -> Option<i32> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    // called from worker threads too, which have no autorelease pool of their own
    objc2::rc::autoreleasepool(|_| unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let front: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if front.is_null() {
            return None;
        }
        let pid: i32 = msg_send![front, processIdentifier];
        Some(pid)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app_pid() -> Option<i32> {
    None
}

/// Whether Yonder is the active app. Call on the main thread.
#[cfg(target_os = "macos")]
pub fn yonder_is_active() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe {
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        !ns_app.is_null() && msg_send![ns_app, isActive]
    }
}

#[cfg(not(target_os = "macos"))]
pub fn yonder_is_active() -> bool {
    false
}

/// Makes the app with this process id active again (only its own front windows come forward).
/// Call on the main thread.
#[cfg(target_os = "macos")]
pub fn activate_app(pid: i32) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe {
        let running: *mut AnyObject = msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
        if !running.is_null() {
            let _: bool = msg_send![running, activateWithOptions: 0usize];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_app(_pid: i32) {}

/// Puts a window back on screen behind every other window, without activating Yonder.
/// Call on the main thread.
#[cfg(target_os = "macos")]
pub fn order_back(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    if let Ok(ns_window) = window.ns_window() {
        let ns_window = ns_window as *mut AnyObject;
        let nil: *mut AnyObject = std::ptr::null_mut();
        unsafe {
            let _: () = msg_send![ns_window, orderBack: nil];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn order_back(window: &tauri::WebviewWindow) {
    let _ = window.show();
}

// -------------------------------------------------------------- screenshots

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// Whether Yonder may capture the screen. Only macOS asks (Screen Recording permission),
/// and it only notices a new grant after Yonder restarts.
#[cfg(target_os = "macos")]
pub fn screen_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

#[cfg(not(target_os = "macos"))]
pub fn screen_access() -> bool {
    true
}

/// Asks for Screen Recording permission. macOS shows its own prompt only the first
/// time; after that it stays silent, so Yonder opens the pane in System Settings.
#[cfg(target_os = "macos")]
pub fn request_screen_access(asked_before: bool) {
    if unsafe { CGRequestScreenCaptureAccess() } || !asked_before {
        return;
    }
    let pane = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
    if let Err(e) = std::process::Command::new("open").arg(pane).spawn() {
        crate::logger::warn(&format!("couldn't open System Settings: {e}"));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request_screen_access(_asked_before: bool) {}

/// A monitor rectangle in physical pixels: x, y, width, height.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub type MonitorRect = crate::monitors::Rect;

#[cfg(any(target_os = "linux", test))]
fn rgb_to_data_url(rgb: &image::RgbImage) -> Option<String> {
    use base64::Engine;
    use image::codecs::jpeg::JpegEncoder;
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, 82).encode_image(rgb).ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf)
    ))
}

/// Cut one image of the whole desktop into per-monitor JPEG data URLs.
/// The image may be scaled relative to the monitor rects (fractional scaling
/// on Wayland), so the rects' bounding box is mapped onto the image.
#[cfg(any(target_os = "linux", test))]
fn crop_to_monitors(img: &image::DynamicImage, rects: &[MonitorRect]) -> Vec<Option<String>> {
    use image::GenericImageView;
    if rects.is_empty() {
        return Vec::new();
    }
    let min_x = rects.iter().map(|r| r.0).min().unwrap_or(0);
    let min_y = rects.iter().map(|r| r.1).min().unwrap_or(0);
    let max_x = rects.iter().map(|r| r.0 + r.2 as i32).max().unwrap_or(1);
    let max_y = rects.iter().map(|r| r.1 + r.3 as i32).max().unwrap_or(1);
    let (iw, ih) = img.dimensions();
    let near = |a: f64| (a - 1.0).abs() < 0.01;
    let mut sx = iw as f64 / (max_x - min_x).max(1) as f64;
    let mut sy = ih as f64 / (max_y - min_y).max(1) as f64;
    if near(sx) {
        sx = 1.0;
    }
    if near(sy) {
        sy = 1.0;
    }
    rects
        .iter()
        .map(|&(x, y, w, h)| {
            let cx = (((x - min_x) as f64 * sx).round() as i64).clamp(0, iw as i64 - 1) as u32;
            let cy = (((y - min_y) as f64 * sy).round() as i64).clamp(0, ih as i64 - 1) as u32;
            let cw = ((w as f64 * sx).round() as u32).min(iw - cx).max(1);
            let ch = ((h as f64 * sy).round() as u32).min(ih - cy).max(1);
            let part = img.crop_imm(cx, cy, cw, ch).to_rgb8();
            rgb_to_data_url(&part)
        })
        .collect()
}

/// One JPEG data URL per requested monitor, in order (`None` where the capture failed).
/// Each entry is a monitor's physical rectangle and DPI scale as the window toolkit
/// reports it; it is matched to a captured display by overlap, never by order, so a
/// monitor left of / above the primary or at another scale gets its own picture.
#[cfg(not(target_os = "linux"))]
pub fn screenshots_for(wanted: &[(crate::monitors::Rect, f64)]) -> Vec<Option<String>> {
    use base64::Engine;
    use image::{codecs::jpeg::JpegEncoder, DynamicImage};

    let Ok(displays) = xcap::Monitor::all() else {
        return vec![None; wanted.len()];
    };
    let rects: Vec<crate::monitors::Rect> = displays
        .iter()
        .map(|m| (m.x().unwrap_or(0), m.y().unwrap_or(0), m.width().unwrap_or(0), m.height().unwrap_or(0)))
        .collect();
    wanted
        .iter()
        .map(|(r, scale)| {
            // xcap reports physical pixels on Windows, but points on macOS (where the
            // toolkit's physical coordinates are points times that monitor's scale).
            let want = if cfg!(target_os = "macos") && *scale > 0.0 {
                (
                    (r.0 as f64 / scale).round() as i32,
                    (r.1 as f64 / scale).round() as i32,
                    (r.2 as f64 / scale).round() as u32,
                    (r.3 as f64 / scale).round() as u32,
                )
            } else {
                *r
            };
            let idx = crate::monitors::best_match(&rects, want)?;
            let img = displays[idx].capture_image().ok()?;
            let rgb = DynamicImage::ImageRgba8(img).to_rgb8();
            let mut buf = Vec::new();
            JpegEncoder::new_with_quality(&mut buf, 82).encode_image(&rgb).ok()?;
            Some(format!(
                "data:image/jpeg;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(buf)
            ))
        })
        .collect()
}

/// Linux: the whole desktop is captured once (X11 GetImage on the root window,
/// or on Wayland the xdg-desktop-portal Screenshot API, then `grim`), then cut
/// per monitor. Any failure yields None for every monitor and the break falls
/// back to its backdrop; nothing here may hang a break.
#[cfg(target_os = "linux")]
pub fn screenshots_for(wanted: &[(crate::monitors::Rect, f64)]) -> Vec<Option<String>> {
    let rects: Vec<MonitorRect> = wanted.iter().map(|w| w.0).collect();
    let rects = &rects[..];
    let started = std::time::Instant::now();
    match linux_shot::capture_desktop() {
        Some(img) => {
            let out = crop_to_monitors(&img, rects);
            crate::logger::info(&format!("screenshot ok in {} ms", started.elapsed().as_millis()));
            out
        }
        None => {
            crate::logger::info("screenshot unavailable on this session; using the backdrop");
            vec![None; rects.len()]
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_shot {
    use image::{DynamicImage, RgbImage};
    use std::{
        collections::HashMap,
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };

    /// How long to wait for the Wayland portal. GNOME may show a one-time
    /// permission dialog; if it is not answered in time the break just goes ahead.
    const PORTAL_TIMEOUT: Duration = Duration::from_millis(1500);

    pub fn capture_desktop() -> Option<DynamicImage> {
        let wayland = std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
            || std::env::var_os("WAYLAND_DISPLAY").is_some();
        if wayland {
            // XWayland would only give black or stale pixels, so no X11 attempt here.
            portal_capture().or_else(grim_capture)
        } else {
            x11_capture()
        }
    }

    /// Grab the root window with core-protocol GetImage, in strips so no single
    /// reply is huge. Only the common 32-bpp little-endian BGRX layout is handled.
    fn x11_capture() -> Option<DynamicImage> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};

        let (conn, num) = x11rb::connect(None).ok()?;
        let setup = conn.setup();
        let screen = &setup.roots[num];
        if setup.image_byte_order != ImageOrder::LSB_FIRST {
            return None;
        }
        let vis = screen
            .allowed_depths
            .iter()
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.visual_id == screen.root_visual)?;
        if vis.red_mask != 0x00ff_0000 || vis.green_mask != 0x0000_ff00 || vis.blue_mask != 0x0000_00ff {
            return None;
        }
        let fmt = setup.pixmap_formats.iter().find(|f| f.depth == screen.root_depth)?;
        if fmt.bits_per_pixel != 32 {
            return None;
        }
        let (w, h) = (screen.width_in_pixels as usize, screen.height_in_pixels as usize);
        if w == 0 || h == 0 {
            return None;
        }
        let rows_per = ((4usize << 20) / (w * 4)).clamp(1, h);
        let mut rgb = Vec::with_capacity(w * h * 3);
        let mut y = 0usize;
        while y < h {
            let n = rows_per.min(h - y);
            let reply = conn
                .get_image(ImageFormat::Z_PIXMAP, screen.root, 0, y as i16, w as u16, n as u16, !0)
                .ok()?
                .reply()
                .ok()?;
            if reply.data.len() < w * n * 4 {
                return None;
            }
            for px in reply.data[..w * n * 4].chunks_exact(4) {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            }
            y += n;
        }
        RgbImage::from_raw(w as u32, h as u32, rgb).map(DynamicImage::ImageRgb8)
    }

    /// org.freedesktop.portal.Screenshot, non-interactive. The result is a
    /// file URI that we read and delete.
    fn portal_capture() -> Option<DynamicImage> {
        let (tx, rx) = mpsc::channel();
        // A thread so a never-answered request (open permission dialog) can't hang us.
        std::thread::spawn(move || {
            let _ = tx.send(portal_request());
        });
        let uri = rx.recv_timeout(PORTAL_TIMEOUT).ok()??;
        let path = uri_to_path(&uri)?;
        let img = image::open(&path).ok();
        let _ = std::fs::remove_file(&path);
        img
    }

    fn portal_request() -> Option<String> {
        use zbus::zvariant::{OwnedValue, Value};
        use zbus::{blocking::Connection, blocking::MessageIterator, message::Type, MatchRule};

        let conn = Connection::session().ok()?;
        let token = format!("yonder{}", std::process::id());
        let sender = conn.unique_name()?.as_str().trim_start_matches(':').replace('.', "_");
        let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
        // Subscribe before calling, otherwise a fast Response could be missed.
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.portal.Request")
            .ok()?
            .member("Response")
            .ok()?
            .path(path.as_str())
            .ok()?
            .build();
        let mut responses = MessageIterator::for_match_rule(rule, &conn, Some(4)).ok()?;
        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("interactive", Value::from(false));
        conn.call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Screenshot"),
            "Screenshot",
            &("", options),
        )
        .ok()?;
        let msg = responses.next()?.ok()?;
        let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().ok()?;
        if code != 0 {
            return None;
        }
        let uri = results.get("uri")?.try_clone().ok()?;
        String::try_from(uri).ok()
    }

    /// wlroots compositors (Sway, Hyprland, ...) often lack the portal screenshot.
    fn grim_capture() -> Option<DynamicImage> {
        let out = Command::new("grim")
            .args(["-t", "png", "-"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        image::load_from_memory(&out.stdout).ok()
    }

    /// `file:///a%20b.png` to `/a b.png`.
    fn uri_to_path(uri: &str) -> Option<String> {
        let rest = uri.strip_prefix("file://")?;
        let bytes = rest.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).ok()
    }
}

#[cfg(test)]
mod shot_tests {
    use super::*;

    #[test]
    fn crops_per_monitor_with_scaling() {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(400, 100));
        // Two 100x50 monitors side by side, image is 2x the logical size.
        let out = crop_to_monitors(&img, &[(0, 0, 100, 50), (100, 0, 100, 50)]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|o| o.as_deref().map_or(false, |s| s.starts_with("data:image/jpeg"))));
        assert!(crop_to_monitors(&img, &[]).is_empty());
    }
}
