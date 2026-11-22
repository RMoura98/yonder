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
    if hr == 0 && matches!(st, 2 | 3 | 4) {
        return false;
    }
    if hr == 0 && st == 5 {
        return true;
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
    std::fs::read_to_string(path)
        .map(|s| parse_macos_assertions(&s))
        .unwrap_or(false)
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

#[cfg(test)]
mod tests {
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
