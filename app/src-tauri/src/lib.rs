mod cli;
mod engine;
mod evening;
mod gamma;
mod haptics;
mod i18n;
mod logger;
mod media;
mod monitors;
mod platform;
mod settings;
mod stats;
mod updates;
mod wayland_idle;

use engine::{Core, Effect, Snapshot};
use logger::{log_debug, log_info, log_warn};
use updates::UpdateInfo;
use settings::{Reminder, Settings};
use stats::Stats;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

struct Paths {
    settings: PathBuf,
    stats: PathBuf,
    /// Countdown snapshot, restored after a quick restart.
    state: PathBuf,
    background: PathBuf,
    /// Folder holding the user's added sounds as data URLs, one file per sound.
    sounds: PathBuf,
    /// Settings screen to return to after macOS restarts Yonder for a new permission.
    resume: PathBuf,
    /// Present once Yonder has asked macOS for Screen Recording permission.
    screen_asked: PathBuf,
    /// Exists while the evening filter has changed the display, so a crash can be cleaned up.
    evening_marker: PathBuf,
}

struct TrayItems {
    menu: Menu<Wry>,
    status: MenuItem<Wry>,
    skip: MenuItem<Wry>,
    /// "Start/Stop focus session"; the label follows the session state.
    focus: MenuItem<Wry>,
    /// "Turn on/off evening filter"; the label follows the filter.
    evening: MenuItem<Wry>,
    pause: Submenu<Wry>,
    resume: MenuItem<Wry>,
    /// "Update available…" entry, inserted at the top only when there is one.
    update: Option<(MenuItem<Wry>, PredefinedMenuItem<Wry>)>,
    /// Static items and their English source text, for re-labelling.
    labeled: Vec<(MenuItem<Wry>, &'static str)>,
}

#[derive(Default)]
struct BreakWins {
    /// Next number for a window label ("break-N"); labels are never reused within a run.
    next: usize,
    /// (label, monitor rectangle, is the primary's window)
    list: Vec<(String, monitors::Rect, bool)>,
}

struct AppState {
    core: Mutex<Core>,
    paths: Paths,
    evening: Mutex<evening::Controller>,
    snapshots: Mutex<HashMap<String, String>>,
    /// The break windows currently open, one per monitor.
    break_wins: Mutex<BreakWins>,
    reminder_queue: Mutex<Vec<Reminder>>,
    tray: Mutex<Option<TrayItems>>,
    /// (light taskbar, paused) the tray glyph was last drawn for
    tray_look: Mutex<Option<TrayLook>>,
    /// Newer release found by the update checker (and not skipped).
    update: Mutex<Option<UpdateInfo>>,
    /// Where the tray icon is (physical x, y, w, h), or the cursor, for placing the popover.
    popover_anchor: Mutex<Option<(f64, f64, f64, f64)>>,
    /// When the popover last closed, so the tray click that blurred it doesn't reopen it.
    popover_closed: Mutex<Option<Instant>>,
    /// The last yonder:// link handled, to drop the duplicate some platforms deliver.
    last_link: Mutex<Option<(String, Instant)>>,
    /// Folder holding the log files.
    log_dir: PathBuf,
    /// macOS: what to restore when a focused break or the popover ends (see `remember_before_focus`).
    before_focus: Mutex<Option<BeforeFocus>>,
}

/// A full-screen break and the tray popover make Yonder the active app (they need the
/// keyboard), and on macOS activating an app brings all its windows forward, Settings
/// included. So before taking focus Yonder notes the app you were in and tucks a background
/// Settings window away; afterwards Settings goes back behind your windows and the keyboard
/// goes back to your app. Opening Settings on purpose cancels all of that.
#[derive(Default)]
struct BeforeFocus {
    /// Process id of the app that was in front (not Yonder itself).
    front_app: Option<i32>,
    /// Settings was open in the background and got hidden for the break.
    settings_hidden: bool,
}

fn remember_before_focus(app: &AppHandle) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let front_app = platform::frontmost_app_pid().filter(|pid| *pid as u32 != std::process::id());
    let mut settings_hidden = false;
    if let Some(w) = app.get_webview_window("settings") {
        if w.is_visible().unwrap_or(false) && !w.is_focused().unwrap_or(false) {
            settings_hidden = w.hide().is_ok();
        }
    }
    let st = state(app);
    let mut before = st.before_focus.lock().unwrap();
    // A break started from the popover (or while another is closing) keeps the first snapshot.
    match before.as_mut() {
        None => *before = Some(BeforeFocus { front_app, settings_hidden }),
        Some(b) => b.settings_hidden |= settings_hidden,
    }
}

fn restore_after_focus(app: &AppHandle) {
    let taken = state(app).before_focus.lock().unwrap().take();
    let Some(before) = taken else { return };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if before.settings_hidden {
            if let Some(w) = handle.get_webview_window("settings") {
                platform::order_back(&w);
            }
        }
        // only if Yonder still has the keyboard: don't yank you out of an app you just picked
        if let (Some(pid), true) = (before.front_app, platform::yonder_is_active()) {
            platform::activate_app(pid);
        }
    });
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

// ================================================================== windows

fn open_settings(app: &AppHandle) {
    // asked for on purpose: don't hide it or hand the keyboard elsewhere after a break or the popover
    state(app).before_focus.lock().unwrap().take();
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()));
    // No native title bar: macOS keeps its traffic lights over the sidebar,
    // Windows/Linux get window controls drawn by the page itself.
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false).shadow(true);
    let _ = builder
        .title("Yonder")
        .inner_size(960.0, 680.0)
        .min_inner_size(780.0, 540.0)
        .center()
        .focused(true)
        // Never navigate the settings window away from the app: web links go to the browser.
        .on_navigation(|url| {
            let internal = matches!(url.scheme(), "tauri" | "asset")
                || url.host_str() == Some("tauri.localhost")
                || url.host_str() == Some("localhost");
            if !internal {
                platform::open_url(url.as_str());
            }
            internal
        })
        .build();
}

fn rect_of(m: &tauri::Monitor) -> monitors::Rect {
    (m.position().x, m.position().y, m.size().width, m.size().height)
}

fn cursor_monitor(app: &AppHandle) -> Option<tauri::Monitor> {
    let pos = app.cursor_position().ok().map(|p| (p.x, p.y));
    monitor_at(app, pos)
}

/// The monitor containing a physical point (the primary one if it is off every screen).
fn monitor_at(app: &AppHandle, pos: Option<(f64, f64)>) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().ok()?;
    if let Some(p) = pos {
        let rects: Vec<_> = monitors.iter().map(rect_of).collect();
        if let Some(i) = monitors::index_at(&rects, p) {
            return Some(monitors[i].clone());
        }
    }
    app.primary_monitor().ok().flatten().or_else(|| monitors.into_iter().next())
}

fn floating_window(app: &AppHandle, label: &str, url: &str, w: f64, h: f64) -> Option<WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title("Yonder")
        .inner_size(w, h)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        // Heads-up, cursor countdown and reminders must never take keyboard focus
        // from whatever you're typing in; clicking their buttons still works.
        .focusable(false)
        .visible(false)
        .build()
        .ok()
}

/// Shows a floating window without activating it (no focus steal).
fn show_quietly(window: &WebviewWindow) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        if let Ok(hwnd) = window.hwnd() {
            unsafe { ShowWindow(hwnd.0 as _, SW_SHOWNOACTIVATE) };
            return;
        }
    }
    // show() calls makeKeyAndOrderFront, which can pull keyboard focus away from the app
    // you're typing in; orderFrontRegardless only puts the window on screen, on top.
    #[cfg(target_os = "macos")]
    {
        let w = window.clone();
        let shown = window.run_on_main_thread(move || {
            if let Ok(ns_window) = w.ns_window() {
                let ns_window = ns_window as *mut objc2::runtime::AnyObject;
                unsafe {
                    let _: () = objc2::msg_send![ns_window, orderFrontRegardless];
                }
            }
        });
        if shown.is_ok() {
            return;
        }
    }
    let _ = window.show();
}

/// Removes the native title bar from a break window for good (Windows).
///
/// Undecorated windows keep WS_CAPTION and only hide it by drawing over the frame;
/// while a borderless game holds the foreground, Windows can paint that caption anyway
/// (a grey strip with "Yonder break" across the top). A plain popup has no caption to
/// paint, so make the window one and re-apply its monitor rectangle.
#[allow(unused_variables)]
fn strip_title_bar(window: &WebviewWindow, pos: PhysicalPosition<i32>, size: PhysicalSize<u32>) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
        };
        let Ok(hwnd) = window.hwnd() else { return };
        let hwnd = hwnd.0 as _;
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            let frame = WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MAXIMIZEBOX | WS_MINIMIZEBOX;
            SetWindowLongW(hwnd, GWL_STYLE, ((style & !frame) | WS_POPUP) as i32);
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                pos.x,
                pos.y,
                size.width as i32,
                size.height as i32,
                SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        }
    }
}

/// Places a floating window horizontally centered (or right-aligned) near the top.
fn place_top(app: &AppHandle, win: &WebviewWindow, align_right: bool) {
    let Some(m) = cursor_monitor(app) else { return };
    let scale = m.scale_factor();
    let size = size_on(win, scale, (400, 200));
    let (x, top) = monitors::top_position(rect_of(&m), scale, size, align_right);
    let _ = win.set_position(PhysicalPosition::new(x, top));
}

/// The window's physical size once it sits on a monitor with DPI scale `to` (it is
/// measured on the monitor it is on now, which may have a different scale).
fn size_on(win: &WebviewWindow, to: f64, fallback: (u32, u32)) -> (u32, u32) {
    let cur = win.outer_size().map(|s| (s.width, s.height)).unwrap_or(fallback);
    monitors::rescale(cur, win.scale_factor().unwrap_or(to), to)
}

/// The heads-up before a break, in the style picked in Settings.
fn show_heads_up(app: &AppHandle) {
    let card = state(app).core.lock().unwrap().settings.prebreak_style == "card";
    if card {
        show_prebreak(app);
    } else {
        show_cursor_pill(app);
    }
}

fn show_prebreak(app: &AppHandle) {
    if app.get_webview_window("prebreak").is_some() {
        return;
    }
    if let Some(w) = floating_window(app, "prebreak", "prebreak.html", 400.0, 140.0) {
        place_top(app, &w, false);
        // shown by the page (fit_window) once it has laid out, so no empty frame flashes
    }
}

/// A tiny countdown pill that trails the mouse: the "cursor" heads-up style, and in
/// every style the last seconds before a break.
fn show_cursor_pill(app: &AppHandle) {
    if app.get_webview_window("cursor").is_some() {
        return;
    }
    let Some(w) = floating_window(app, "cursor", "cursor.html", 210.0, 48.0) else { return };
    let _ = w.set_ignore_cursor_events(true);
    let follow = |app: &AppHandle, w: &WebviewWindow| {
        if let Ok(p) = app.cursor_position() {
            let scale = w.scale_factor().unwrap_or(1.0);
            let _ = w.set_position(PhysicalPosition::new(
                // the pill sits 20px inside its window (room for its shadow)
                (p.x + 5.0 * scale) as i32,
                (p.y + 9.0 * scale) as i32,
            ));
        }
    };
    follow(app, &w);
    // shown by the page (fit_window) once it has laid out
    let app = app.clone();
    std::thread::spawn(move || {
        // ~60 fps while visible; the window closes when the break starts.
        while let Some(w) = app.get_webview_window("cursor") {
            follow(&app, &w);
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}

// ================================================================== popover

const POPOVER_W: f64 = 332.0; // 300px card + room for its shadow

/// Opens or closes the tray popover. `anchor` is the tray icon rect (physical px);
/// without one the popover appears at the mouse pointer.
fn toggle_popover(app: &AppHandle, anchor: Option<(f64, f64, f64, f64)>) {
    if app.get_webview_window("popover").is_some() {
        close_window(app, "popover");
        return;
    }
    // The click that blurred (and so closed) the popover must not reopen it. Tray clicks wait
    // up to double_click_ms before toggling (to spot double clicks), so the guard covers that too.
    let guard = Duration::from_millis(350 + platform::double_click_ms().min(350));
    let recent = state(app).popover_closed.lock().unwrap().map_or(false, |t| t.elapsed() < guard);
    if !recent {
        show_popover(app, anchor);
    }
}

fn show_popover(app: &AppHandle, anchor: Option<(f64, f64, f64, f64)>) {
    if let Some(w) = app.get_webview_window("popover") {
        let _ = w.set_focus();
        return;
    }
    remember_before_focus(app);
    let anchor = anchor.or_else(|| app.cursor_position().ok().map(|p| (p.x, p.y, 0.0, 0.0)));
    *state(app).popover_anchor.lock().unwrap() = anchor;
    let Ok(w) = WebviewWindowBuilder::new(app, "popover", WebviewUrl::App("popover.html".into()))
        .title("Yonder")
        .inner_size(POPOVER_W, 420.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()
    else {
        restore_after_focus(app);
        return;
    };
    // Close when it loses focus (a click anywhere else). Ignore a blur that arrives
    // before it ever had focus.
    let had_focus = Arc::new(AtomicBool::new(false));
    let handle = app.clone();
    w.on_window_event(move |ev| match ev {
        tauri::WindowEvent::Focused(true) => had_focus.store(true, Ordering::SeqCst),
        tauri::WindowEvent::Focused(false) if had_focus.load(Ordering::SeqCst) => {
            let h = handle.clone();
            std::thread::spawn(move || close_window(&h, "popover"));
        }
        tauri::WindowEvent::Destroyed => {
            *state(&handle).popover_closed.lock().unwrap() = Some(Instant::now());
            // A full-screen break started from the popover gives focus back when it ends
            // instead. The button's action can land just after the close, so wait a moment.
            let h = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                let focused_break = {
                    let st = state(&h);
                    let c = st.core.lock().unwrap();
                    c.e.brk.is_some() && !matches!(c.settings.break_style.as_str(), "gentle" | "notify")
                };
                if !focused_break {
                    restore_after_focus(&h);
                }
            });
        }
        _ => {}
    });
    // shown (and focused) by the page via fit_window once it has laid out
}

/// Puts the popover next to the tray icon: above it when the tray is in the lower half
/// of its screen (Windows taskbar), below it otherwise (macOS menu bar), clamped on screen.
fn place_popover(app: &AppHandle, win: &WebviewWindow) {
    let anchor = *state(app).popover_anchor.lock().unwrap();
    let Some((ax, ay, aw, ah)) = anchor else { return place_top(app, win, true) };
    let Some(m) = monitor_at(app, Some((ax + aw / 2.0, ay + ah / 2.0))) else { return };
    let scale = m.scale_factor();
    let size = size_on(win, scale, (332, 400));
    let (mp, ms) = (m.position(), m.size());
    let (w, h) = (size.0 as f64, size.1 as f64);
    let (mx, my, mw, mh) = (mp.x as f64, mp.y as f64, ms.width as f64, ms.height as f64);
    // the page keeps 16px of shadow room around the card; the card sits 8px off the icon
    let off = 8.0 * scale;
    let y = if ay + ah / 2.0 > my + mh / 2.0 { ay - h + off } else { ay + ah - off };
    let x = ax + aw / 2.0 - w / 2.0;
    let edge = 4.0 * scale;
    let x = x.clamp(mx + edge, (mx + mw - w - edge).max(mx + edge));
    let y = y.clamp(my, (my + mh - h).max(my));
    let _ = win.set_position(PhysicalPosition::new(x as i32, y as i32));
}

fn close_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.destroy();
    }
}

fn break_notice_body(lang: &str, kind: &str, total_secs: u64) -> String {
    if kind == "short" && total_secs < 60 {
        i18n::tr(lang, "Time for a {n}-second break — look at something far away", &[("n", &total_secs.to_string())])
    } else {
        let n = ((total_secs + 30) / 60).max(1).to_string();
        if kind == "long" {
            i18n::tr(lang, "Time for a {n}-minute break — stand up, stretch, refill your water", &[("n", &n)])
        } else {
            i18n::tr(lang, "Time for a {n}-minute break — look at something far away", &[("n", &n)])
        }
    }
}

/// "Notification only" style: no window at all, just a native notification plus the
/// break sound (played by a tiny hidden page, since sounds are Web Audio).
fn notify_break(app: &AppHandle) {
    let (lang, brk, sound) = {
        let st = state(app);
        let c = st.core.lock().unwrap();
        (i18n::resolve(&c.settings.language), c.e.brk.clone(), c.settings.sound_enabled)
    };
    if let Some(b) = brk {
        let body = break_notice_body(lang, &b.kind, b.total_secs);
        let _ = app.notification().builder().title("Yonder").body(body).show();
    }
    if sound && app.get_webview_window("break-sound").is_none() {
        // closed with the other "break-" windows when the break ends
        let _ = WebviewWindowBuilder::new(app, "break-sound", WebviewUrl::App("sound.html".into()))
            .title("Yonder sound")
            .focusable(false)
            .focused(false)
            .skip_taskbar(true)
            .visible(false)
            .build();
    }
}

fn start_break_windows(app: &AppHandle) {
    // The heads-up card and the cursor countdown go away when a break starts, and must be
    // gone from the screen before we photograph the desktop.
    let overlays: Vec<&str> = ["prebreak", "cursor"].into_iter().filter(|l| app.get_webview_window(l).is_some()).collect();
    for label in &overlays {
        close_window(app, label);
    }
    if state(app).core.lock().unwrap().settings.break_style == "notify" {
        return notify_break(app);
    }
    let mons = match app.available_monitors() {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => return log_warn!("no monitors reported for the break"),
        Err(e) => return log_warn!("couldn't list monitors for the break: {e}"),
    };
    let needs_shot = {
        let st = state(app);
        let s = &st.core.lock().unwrap().settings;
        s.break_style == "split" || (s.break_style != "gentle" && s.background == "blur")
    };
    let primary = primary_rect(app, &mons);
    log_info!(
        "break on {} monitor(s): {}",
        mons.len(),
        mons.iter()
            .map(|m| {
                let r = rect_of(m);
                format!(
                    "{}x{} at {},{} scale {:.2}{}",
                    r.2,
                    r.3,
                    r.0,
                    r.1,
                    m.scale_factor(),
                    if r == primary { " (primary)" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    );

    // a fresh break starts from a clean slate (stale windows from a crashed break included)
    {
        let st = state(app);
        let mut bw = st.break_wins.lock().unwrap();
        for (label, _, _) in bw.list.drain(..) {
            close_window(app, &label);
        }
    }

    // gentle breaks never take focus, so there is nothing to give back afterwards (macOS)
    if state(app).core.lock().unwrap().settings.break_style != "gentle" {
        remember_before_focus(app);
    }

    let mut shots: Vec<Option<String>> = vec![None; mons.len()];
    if needs_shot {
        if !overlays.is_empty() {
            std::thread::sleep(Duration::from_millis(150));
        }
        let wanted: Vec<_> = mons.iter().map(|m| (rect_of(m), m.scale_factor())).collect();
        shots = platform::screenshots_for(&wanted);
        state(app).snapshots.lock().unwrap().clear();
    }

    for (m, shot) in mons.iter().zip(shots) {
        open_break_window(app, m, rect_of(m) == primary, shot);
    }
}

/// The primary monitor's rectangle (the first monitor if the OS names none).
fn primary_rect(app: &AppHandle, mons: &[tauri::Monitor]) -> monitors::Rect {
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|p| rect_of(&p))
        .or_else(|| mons.first().map(rect_of))
        .unwrap_or((0, 0, 0, 0))
}

/// Opens the break overlay for one monitor. Only the primary one carries the controls
/// and the sound (`?primary=1`) and takes focus; `shot` is that monitor's own desktop picture.
fn open_break_window(app: &AppHandle, m: &tauri::Monitor, is_primary: bool, shot: Option<String>) {
    let gentle = state(app).core.lock().unwrap().settings.break_style == "gentle";
    let label = {
        let st = state(app);
        let mut bw = st.break_wins.lock().unwrap();
        let label = format!("break-{}", bw.next);
        bw.next += 1;
        label
    };
    let url = format!("break.html?primary={}", if is_primary { 1 } else { 0 });
    if let Some(s) = shot {
        state(app).snapshots.lock().unwrap().insert(label.clone(), s);
    }
    let Ok(w) = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .focusable(!gentle)
        .title("Yonder break")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible_on_all_workspaces(true)
        .visible(false)
        .build()
    else {
        log_warn!("couldn't open the break window {label}");
        state(app).snapshots.lock().unwrap().remove(&label);
        return;
    };
    state(app).break_wins.lock().unwrap().list.push((label, rect_of(m), is_primary));
    // The window starts on the primary monitor; moving it to a monitor with another
    // DPI scale can rescale it, so place, size, then place again.
    let _ = w.set_position(*m.position());
    let _ = w.set_size(*m.size());
    let _ = w.set_position(*m.position());
    // Native fullscreen covers the monitor pixel-exactly (no stray borders), which
    // keeps the split-screen illusion seamless. macOS fullscreen would open a new
    // Space with an animation, so there we keep the borderless, monitor-sized window.
    // Gentle mode is a click-through veil: no fullscreen (keeps the taskbar and the
    // user's apps undisturbed), never focused, and the mouse passes straight through.
    if gentle {
        let _ = w.set_ignore_cursor_events(true);
        strip_title_bar(&w, *m.position(), *m.size());
        show_quietly(&w);
        return;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = w.set_fullscreen(true);
    strip_title_bar(&w, *m.position(), *m.size());
    let _ = w.show();
    if is_primary {
        let _ = w.set_focus();
    }
}

/// Keeps the break overlays in step with the monitors while a break is showing: a
/// monitor that was unplugged loses its window, a newly attached one gets one (without
/// a desktop picture: the overlays would be in the shot, so it uses the plain backdrop).
/// If the primary changed, its window is reopened so the controls follow it.
fn sync_break_windows(app: &AppHandle) {
    let Ok(mons) = app.available_monitors() else { return };
    if mons.is_empty() {
        return; // mid-reconfiguration; try again next tick
    }
    let primary = primary_rect(app, &mons);
    let now: Vec<monitors::Rect> = mons.iter().map(rect_of).collect();
    let (have, had_primary): (Vec<monitors::Rect>, Option<monitors::Rect>) = {
        let st = state(app);
        let bw = st.break_wins.lock().unwrap();
        (bw.list.iter().map(|e| e.1).collect(), bw.list.iter().find(|e| e.2).map(|e| e.1))
    };
    if have.is_empty() {
        return; // no overlays (notification style, or the break just ended)
    }
    let (gone, added) = monitors::diff(&have, &now);
    let primary_moved = had_primary != Some(primary) && now.contains(&primary) && have.contains(&primary);
    if gone.is_empty() && added.is_empty() && !primary_moved {
        return;
    }
    log_info!("monitors changed during the break: {} closed, {} added", gone.len(), added.len());
    let mut reopen: Vec<monitors::Rect> = added.iter().map(|&i| now[i]).collect();
    {
        let st = state(app);
        let mut bw = st.break_wins.lock().unwrap();
        let mut keep = Vec::new();
        for (label, r, was_primary) in bw.list.drain(..) {
            let stale = !now.contains(&r) || (was_primary != (r == primary));
            if stale {
                close_window(app, &label);
                st.snapshots.lock().unwrap().remove(&label);
                if now.contains(&r) {
                    reopen.push(r); // same monitor, new role
                }
            } else {
                keep.push((label, r, was_primary));
            }
        }
        bw.list = keep;
    }
    for r in reopen {
        if let Some(m) = mons.iter().find(|m| rect_of(m) == r) {
            open_break_window(app, m, r == primary, None);
        }
    }
}

fn end_break_windows(app: &AppHandle, delay_ms: u64) {
    let _ = app.emit("break-ending", delay_ms);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay_ms));
        let labels: Vec<String> = app
            .webview_windows()
            .keys()
            .filter(|l| l.starts_with("break-"))
            .cloned()
            .collect();
        for l in labels {
            close_window(&app, &l);
        }
        state(&app).snapshots.lock().unwrap().clear();
        state(&app).break_wins.lock().unwrap().list.clear();
        restore_after_focus(&app);
    });
}

fn show_reminders(app: &AppHandle, list: Vec<Reminder>) {
    state(app).reminder_queue.lock().unwrap().extend(list);
    if app.get_webview_window("reminders").is_some() {
        let _ = app.emit_to("reminders", "reminders-updated", ());
    } else if let Some(w) = floating_window(app, "reminders", "reminders.html", 380.0, 120.0) {
        place_top(app, &w, false);
        // shown by the page once it has laid out its content
    }
}

// ==================================================================== effects

fn apply(app: &AppHandle, fx: Vec<Effect>) {
    for f in fx {
        match f {
            Effect::ShowPrebreak => {
                log_debug!("heads-up shown");
                show_heads_up(app)
            }
            Effect::HidePrebreak => {
                close_window(app, "prebreak");
                close_window(app, "cursor");
            }
            Effect::StartBreak => {
                {
                    let st = state(app);
                    let c = st.core.lock().unwrap();
                    if let Some(b) = &c.e.brk {
                        log_info!("break started: {} ({}s, style {})", b.kind, b.total_secs, c.settings.break_style);
                    }
                }
                // (start_break_windows closes the cursor countdown before its screenshot)
                if state(app).core.lock().unwrap().settings.pause_media {
                    media::pause_playing();
                }
                start_break_windows(app)
            }
            Effect::EndBreak { delay_ms } => {
                log_info!("break ended");
                // Resume only what we paused, even if the setting was turned off mid-break.
                media::resume_paused();
                end_break_windows(app, delay_ms)
            }
            Effect::ShowReminders(list) => show_reminders(app, list),
            Effect::NotifyDue { kind, secs } => {
                let lang = i18n::resolve(&state(app).core.lock().unwrap().settings.language);
                let body = break_notice_body(lang, &kind, secs);
                let _ = app.notification().builder().title("Yonder").body(body).show();
            }
            Effect::Haptic(cue) => haptics::play(app, cue),
            Effect::Lock => platform::lock_screen(),
            Effect::Hook { cmd, event, kind, secs } => platform::run_hook(&cmd, event, &kind, secs),
            Effect::SaveStats => {
                let st = state(app);
                let stats = st.core.lock().unwrap().stats.clone();
                stats.save(&st.paths.stats);
            }
        }
    }
    broadcast(app);
}

fn broadcast(app: &AppHandle) {
    let (snap, heads_up) = {
        let st = state(app);
        let core = st.core.lock().unwrap();
        (core.snapshot(), core.settings.prebreak_enabled)
    };
    // Final seconds before a break: a countdown pill follows the cursor.
    if heads_up && snap.phase == "working" && (1..=5).contains(&snap.next_break_in) {
        show_cursor_pill(app);
    }
    update_tray(app, &snap);
    if !app.webview_windows().is_empty() {
        let _ = app.emit("state", &snap);
    }
}

/// The status line for the tray menu: the first sentence of the label, without its
/// full stop ("Break due — waiting while you're in a call (Discord)").
fn menu_status(label: &str) -> String {
    let first = match label.find(". ") {
        Some(i) => &label[..i],
        None => label.trim_end_matches('.'),
    };
    // CJK sentences end with "。"
    let first = first.split('。').next().unwrap_or(first);
    first.to_string()
}

/// What the tray icon currently shows: light taskbar, paused, and the countdown drawn into it (Windows).
type TrayLook = (bool, bool, Option<(String, bool)>);

fn update_tray(app: &AppHandle, snap: &Snapshot) {
    let lang = ui_lang(app);
    let st = state(app);
    let guard = st.tray.lock().unwrap();
    let Some(t) = guard.as_ref() else { return };
    // the menu shows only the first sentence: the full hold explanation made it very wide
    let _ = t.status.set_text(menu_status(&snap.label));
    let paused = snap.phase == "paused";
    let _ = t.resume.set_enabled(paused);
    let _ = t.pause.set_enabled(!paused);
    let _ = t.skip.set_enabled(snap.skip_difficulty != "hardcore");
    let _ = t.focus.set_text(i18n::tr(
        lang,
        if snap.focus.is_some() { "Stop focus session" } else { "Start focus session" },
        &[],
    ));
    if let Some(tray) = app.tray_by_id("main") {
        // Windows can't show text beside a tray icon, so the countdown is drawn into the icon itself
        // (opt-in). Only while counting down; breaks and pauses keep the normal glyph.
        let in_icon = cfg!(windows)
            && matches!(snap.phase, "working" | "smart" | "away")
            && st.core.lock().unwrap().settings.tray_icon_countdown;
        let count = in_icon.then(|| platform::countdown_label(snap.next_break_in));
        // Re-draw only when the taskbar theme, the paused state or the shown value changes.
        let look: TrayLook = (platform::taskbar_is_light(), paused, count);
        let mut last = st.tray_look.lock().unwrap();
        if last.as_ref() != Some(&look) {
            let icon = match &look.2 {
                Some((text, secs)) => tray_countdown_icon(look.0, text, *secs),
                None => tray_icon(look.0, look.1),
            };
            *last = Some(look);
            let _ = tray.set_icon(Some(icon));
            let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
        }
        drop(last);
        let _ = tray.set_tooltip(Some(format!("Yonder — {}", menu_status(&snap.label))));
        // Text next to the tray icon (macOS menu bar, some Linux panels; ignored on Windows).
        // Icon-only when the user turned the countdown off (the tooltip still has it).
        let show = st.core.lock().unwrap().settings.tray_countdown;
        let title = match snap.phase {
            _ if !show => String::new(),
            "break" => i18n::tr(lang, "Break", &[]),
            "paused" => i18n::tr(lang, "Paused", &[]),
            "working" | "smart" | "away" => {
                let s = snap.next_break_in.max(0) as u64;
                if s >= 60 { format!("{}m", (s + 59) / 60) } else { format!("0:{:02}", s) }
            }
            _ => String::new(),
        };
        let _ = tray.set_title(Some(title));
    }
}

fn with_core<F: FnOnce(&mut Core) -> Vec<Effect>>(app: &AppHandle, f: F) {
    let fx = {
        let st = state(app);
        let mut core = st.core.lock().unwrap();
        f(&mut core)
    };
    apply(app, fx);
}

// ================================================================== shortcuts

fn register_shortcuts(app: &AppHandle) {
    let sc = state(app).core.lock().unwrap().settings.shortcuts.clone();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let bind = |accel: &str, action: &'static str| {
        if accel.trim().is_empty() {
            return;
        }
        let res = gs.on_shortcut(accel.trim(), move |app, _s, ev| {
            if ev.state == ShortcutState::Pressed {
                log_debug!("shortcut pressed: {action}");
                run_action(app, action, None);
            }
        });
        if let Err(e) = res {
            log_warn!("couldn't register shortcut '{}' for {action}: {e}", accel.trim());
        }
    };
    bind(&sc.take_break, "take_break");
    bind(&sc.toggle_pause, "toggle_pause");
    bind(&sc.skip_break, "skip");
    bind(&sc.toggle_focus, "toggle_focus");
}

// =================================================================== actions

fn run_action(app: &AppHandle, name: &str, arg: Option<i64>) {
    match name {
        "reminder_done" | "open_settings" | "test_prebreak" | "test_reminder" => log_debug!("action: {name}"),
        _ => log_info!("action: {name}{}", arg.map(|a| format!(" {a}")).unwrap_or_default()),
    }
    match name {
        "take_break" => with_core(app, |c| c.take_break_now(arg.map(|a| a as u32))),
        "skip" => with_core(app, |c| c.skip_next()),
        "skip_held" => with_core(app, |c| c.skip_held()),
        "end_break" => with_core(app, |c| c.skip_or_end_break()),
        "snooze" => with_core(app, |c| c.snooze(arg.unwrap_or(5) as u32)),
        "start_focus" => with_core(app, |c| {
            c.start_focus_for(arg.map(|a| a as u32));
            vec![]
        }),
        "stop_focus" => with_core(app, |c| c.stop_focus()),
        "toggle_focus" => with_core(app, |c| c.toggle_focus()),
        "toggle_evening" => {
            state(app).evening.lock().unwrap().toggle_manual();
            evening_tick(app);
        }
        "evening_preview" => {
            state(app).evening.lock().unwrap().start_preview(Instant::now());
            evening_tick(app);
        }
        "start_now" => with_core(app, |c| c.start_now()),
        "pause" => with_core(app, |c| c.pause(arg.filter(|a| *a > 0))),
        "pause_tomorrow" => with_core(app, |c| c.pause_until_tomorrow()),
        "resume" => with_core(app, |c| {
            c.resume();
            vec![]
        }),
        "toggle_pause" => with_core(app, |c| {
            if c.e.pause.is_some() {
                c.resume();
                vec![]
            } else {
                c.pause(None)
            }
        }),
        "reset_timer" => with_core(app, |c| {
            c.e.work_secs = 0;
            c.e.snooze_secs = 0;
            c.e.prebreak_shown = false;
            vec![Effect::HidePrebreak]
        }),
        "reminder_done" => with_core(app, |c| {
            c.reminder_done();
            vec![]
        }),
        "lock" => platform::lock_screen(),
        "test_prebreak" => show_heads_up(app),
        "test_reminder" => {
            let r = state(app).core.lock().unwrap().settings.wellness.posture.clone();
            show_reminders(app, vec![r]);
        }
        "open_settings" => open_settings(app),
        "quit" => {
            let st = state(app);
            let core = st.core.lock().unwrap();
            core.stats.save(&st.paths.stats);
            core.settings.save(&st.paths.settings);
            core.saved_state().save(&st.paths.state);
            drop(core);
            app.exit(0);
        }
        _ => {}
    }
}

// ============================================================== scripting

/// Runs what a command line or a yonder:// link asked for; unknown input is only logged.
fn run_commands(app: &AppHandle, source: &str, parsed: &cli::Parsed) {
    for bad in &parsed.ignored {
        log_warn!("ignored {source} command: {bad}");
    }
    for c in &parsed.commands {
        log_info!("{source} command: {}{}", c.action, c.arg.map(|a| format!(" {a}")).unwrap_or_default());
        run_action(app, c.action, c.arg);
    }
}

/// Handles `yonder://` links. The same link can arrive twice (arguments and an OS event).
fn run_links(app: &AppHandle, urls: Vec<String>) {
    let key = urls.join(" ");
    {
        let st = state(app);
        let mut last = st.last_link.lock().unwrap();
        if matches!(&*last, Some((k, t)) if *k == key && t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some((key, Instant::now()));
    }
    run_commands(app, "link", &cli::parse_args(&urls));
}

// ================================================================== commands

#[tauri::command]
async fn get_settings(app: AppHandle) -> Settings {
    state(&app).core.lock().unwrap().settings.clone()
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    apply_settings(&app, settings);
    Ok(())
}

/// Stores new settings and refreshes everything that depends on them.
fn apply_settings(app: &AppHandle, settings: Settings) {
    let launch = settings.launch_at_login;
    if settings.verbose_logging != state(app).core.lock().unwrap().settings.verbose_logging {
        log_info!("verbose logging {}", if settings.verbose_logging { "on" } else { "off" });
        logger::set_verbose(settings.verbose_logging || std::env::args().any(|a| a == "--verbose"));
    }
    {
        let st = state(app);
        let mut core = st.core.lock().unwrap();
        // keep at most five custom reminders
        let mut s = settings;
        s.wellness.custom.truncate(5);
        s.save(&st.paths.settings);
        core.settings = s;
    }
    register_shortcuts(app);
    relabel_tray(app);
    let al = app.autolaunch();
    if launch != al.is_enabled().unwrap_or(false) {
        if let Err(e) = if launch { al.enable() } else { al.disable() } {
            log_warn!("couldn't {} launch at login: {e}", if launch { "enable" } else { "disable" });
        }
    }
    broadcast(app);
}

#[tauri::command]
async fn get_state(app: AppHandle) -> Snapshot {
    state(&app).core.lock().unwrap().snapshot()
}

#[tauri::command]
async fn get_stats(app: AppHandle) -> Stats {
    state(&app).core.lock().unwrap().stats.clone()
}

#[tauri::command]
async fn reset_stats(app: AppHandle) {
    let st = state(&app);
    let mut core = st.core.lock().unwrap();
    core.stats = Stats::default();
    core.stats.save(&st.paths.stats);
}

#[tauri::command]
async fn action(app: AppHandle, name: String, arg: Option<i64>) {
    run_action(&app, &name, arg);
}

#[tauri::command]
async fn get_screenshot(window: WebviewWindow, app: AppHandle) -> Option<String> {
    state(&app).snapshots.lock().unwrap().get(window.label()).cloned()
}

#[tauri::command]
async fn save_background(app: AppHandle, data_url: String) -> Result<(), String> {
    let st = state(&app);
    if data_url.len() > 25_000_000 {
        return Err("Image is too large (max ~18 MB).".into());
    }
    if let Some(dir) = st.paths.background.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&st.paths.background, data_url).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_background(app: AppHandle) -> Option<String> {
    std::fs::read_to_string(&state(&app).paths.background).ok()
}

// ---------------------------------------------------------- backup / restore

/// Settings plus the user's own sounds and background, as one JSON document.
#[tauri::command]
async fn export_settings(app: AppHandle) -> Result<String, String> {
    let st = state(&app);
    let settings = st.core.lock().unwrap().settings.clone();
    let background = if settings.has_image { std::fs::read_to_string(&st.paths.background).ok() } else { None };
    let sounds = settings
        .custom_sounds
        .iter()
        .filter_map(|c| {
            let data_url = std::fs::read_to_string(st.paths.sounds.join(format!("{}.txt", c.id))).ok()?;
            Some(settings::BackupSound { id: c.id.clone(), name: c.name.clone(), data_url })
        })
        .collect();
    let backup = settings::Backup {
        yonder_backup: settings::BACKUP_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        settings,
        background,
        sounds,
    };
    serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())
}

/// Validates a backup file and says what it contains, without applying anything.
#[tauri::command]
async fn check_backup(text: String) -> Result<serde_json::Value, String> {
    let b = settings::parse_backup(&text)?;
    Ok(serde_json::json!({
        "app_version": b.app_version,
        "sounds": b.sounds.len(),
        "image": b.background.is_some(),
    }))
}

/// Replaces the current settings, custom sounds and background with a backup's.
#[tauri::command]
async fn import_settings(app: AppHandle, text: String) -> Result<Settings, String> {
    let b = settings::parse_backup(&text)?;
    let st = state(&app);
    if let Some(dir) = st.paths.background.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match &b.background {
        Some(data) => std::fs::write(&st.paths.background, data).map_err(|e| e.to_string())?,
        None => {
            let _ = std::fs::remove_file(&st.paths.background);
        }
    }
    // The backup's sound library replaces ours.
    if let Ok(dir) = std::fs::read_dir(&st.paths.sounds) {
        for f in dir.flatten() {
            if f.path().extension().map(|e| e == "txt").unwrap_or(false) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    if !b.sounds.is_empty() {
        std::fs::create_dir_all(&st.paths.sounds).map_err(|e| e.to_string())?;
    }
    for c in &b.sounds {
        std::fs::write(st.paths.sounds.join(format!("{}.txt", c.id)), &c.data_url).map_err(|e| e.to_string())?;
    }
    apply_settings(&app, b.settings);
    Ok(state(&app).core.lock().unwrap().settings.clone())
}

// ------------------------------------------------------------ custom sounds

use settings::valid_sound_id;

/// Stores an audio file (as a data URL) and adds it to the user's sound library.
#[tauri::command]
async fn add_custom_sound(app: AppHandle, name: String, data_url: String) -> Result<settings::CustomSound, String> {
    if !data_url.starts_with("data:audio/") && !data_url.starts_with("data:video/") {
        return Err("That doesn't look like an audio file.".into());
    }
    if data_url.len() > 11_500_000 {
        return Err("That file is too large (max 8 MB).".into());
    }
    let st = state(&app);
    let id = format!("s{}", chrono::Utc::now().timestamp_millis());
    std::fs::create_dir_all(&st.paths.sounds).map_err(|e| e.to_string())?;
    std::fs::write(st.paths.sounds.join(format!("{id}.txt")), data_url).map_err(|e| e.to_string())?;
    let name = name.trim().chars().take(60).collect::<String>();
    let sound = settings::CustomSound { id, name: if name.is_empty() { "Sound".into() } else { name } };
    let mut core = st.core.lock().unwrap();
    core.settings.custom_sounds.push(sound.clone());
    core.settings.save(&st.paths.settings);
    Ok(sound)
}

#[tauri::command]
async fn get_custom_sound(app: AppHandle, id: String) -> Option<String> {
    if !valid_sound_id(&id) {
        return None;
    }
    std::fs::read_to_string(state(&app).paths.sounds.join(format!("{id}.txt"))).ok()
}

/// Deletes a sound; anything that used it falls back to a built-in sound.
#[tauri::command]
async fn remove_custom_sound(app: AppHandle, id: String) -> Result<(), String> {
    if !valid_sound_id(&id) {
        return Err("Unknown sound.".into());
    }
    let st = state(&app);
    let _ = std::fs::remove_file(st.paths.sounds.join(format!("{id}.txt")));
    let key = format!("file:{id}");
    let mut core = st.core.lock().unwrap();
    let s = &mut core.settings;
    s.custom_sounds.retain(|c| c.id != id);
    if s.sound == key {
        s.sound = "chime".into();
    }
    let w = &mut s.wellness;
    for r in std::iter::once(&mut w.posture).chain(std::iter::once(&mut w.blink)).chain(w.custom.iter_mut()) {
        if r.sound == key {
            r.sound = "none".into();
        }
    }
    s.save(&st.paths.settings);
    Ok(())
}

#[tauri::command]
async fn take_reminders(app: AppHandle) -> Vec<Reminder> {
    std::mem::take(&mut *state(&app).reminder_queue.lock().unwrap())
}

#[tauri::command]
async fn fit_window(window: WebviewWindow, width: f64, height: f64, align: Option<String>) {
    let app = window.app_handle().clone();
    if height <= 0.0 {
        let _ = window.destroy();
        return;
    }
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
    match window.label() {
        "cursor" => {}
        "popover" => place_popover(&app, &window),
        _ => place_top(&app, &window, align.as_deref() == Some("right")),
    }
    if window.label() != "cursor" {
        // Moving to a monitor with another DPI scale can rescale the window: restate the size.
        let _ = window.set_size(tauri::LogicalSize::new(width, height));
    }
    if window.label() == "popover" {
        // the one floating window that takes focus: you just clicked the tray for it
        if !window.is_visible().unwrap_or(true) {
            let _ = window.show();
        }
        let _ = window.set_focus();
    } else if !window.is_visible().unwrap_or(true) {
        // only the small floating windows go through here; never let them take focus
        show_quietly(&window);
    }
}

#[tauri::command]
async fn open_url(url: String) {
    platform::open_url(&url);
}

#[tauri::command]
fn platform_name() -> &'static str {
    std::env::consts::OS
}

// ======================================================================= tray

/// The one-color horizon glyph, tinted for the current taskbar / menu bar.
fn tray_icon(light_taskbar: bool, paused: bool) -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    // macOS uses a template image (system tints it); elsewhere pick contrast.
    let rgb = if cfg!(target_os = "macos") || light_taskbar { [0, 0, 0] } else { [255, 255, 255] };
    let alpha = if paused { 0.45 } else { 1.0 };
    tauri::image::Image::new_owned(platform::tray_glyph(SIZE, rgb, alpha), SIZE, SIZE)
}

/// The tray icon with the time until the next break drawn as digits (Windows).
fn tray_countdown_icon(light_taskbar: bool, text: &str, seconds: bool) -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    let rgb = if light_taskbar { [0, 0, 0] } else { [255, 255, 255] };
    tauri::image::Image::new_owned(platform::countdown_glyph(SIZE, text, seconds, rgb), SIZE, SIZE)
}

/// The UI language for text the core draws itself (tray menu, status line).
fn ui_lang(app: &AppHandle) -> &'static str {
    i18n::resolve(&state(app).core.lock().unwrap().settings.language)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let lang = ui_lang(app);
    let item = |id: &str, key: &'static str, enabled: bool| -> tauri::Result<(MenuItem<Wry>, &'static str)> {
        Ok((MenuItem::with_id(app, id, i18n::tr(lang, key, &[]), enabled, None::<&str>)?, key))
    };
    let status = MenuItem::with_id(app, "status", i18n::tr(lang, "Starting…", &[]), false, None::<&str>)?;
    let take = item("take_break", "Take a break now", true)?;
    let skip = item("skip", "Skip next break", true)?;
    let focus = item("toggle_focus", "Start focus session", true)?;
    let evening = item("toggle_evening", "Turn on evening filter", true)?;
    let p30 = item("pause_30", "For 30 minutes", true)?;
    let p60 = item("pause_60", "For 1 hour", true)?;
    let p120 = item("pause_120", "For 2 hours", true)?;
    let ptomorrow = item("pause_tomorrow", "Until tomorrow", true)?;
    let pforever = item("pause_forever", "Until I resume", true)?;
    let pause = Submenu::with_id_and_items(
        app,
        "pause_menu",
        i18n::tr(lang, "Pause", &[]),
        true,
        &[&p30.0, &p60.0, &p120.0, &ptomorrow.0, &pforever.0],
    )?;
    let resume = item("resume", "Resume", false)?;
    let show_status = item("show_status", "Show status", true)?;
    let settings_item = item("open_settings", "Settings…", true)?;
    let quit = item("quit", "Quit Yonder", true)?;
    let (settings, quit_item) = (&settings_item.0, &quit.0);
    let (sep1, sep2) = (PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?);
    let linux = cfg!(target_os = "linux");
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&status, &sep1];
    // Many AppIndicator hosts never report tray clicks, so Linux opens the popover from the menu.
    if linux {
        items.push(&show_status.0);
    }
    items.extend([&take.0 as &dyn IsMenuItem<Wry>, &skip.0, &focus.0, &evening.0, &pause, &resume.0, &sep2, settings, quit_item]);
    let menu = Menu::with_items(app, &items)?;

    TrayIconBuilder::with_id("main")
        .icon(tray_icon(platform::taskbar_is_light(), false))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Yonder")
        .menu(&menu)
        // Left click opens the popover (right click keeps the menu); Linux hosts keep the menu.
        .show_menu_on_left_click(linux)
        .on_tray_icon_event(|tray, ev| {
            // A double click opens the settings window. Windows reports it as
            // click, double click, click, so the clicks around it must not toggle the popover.
            // The first click of a double click must not open the popover: it would finish
            // loading after Settings opened and take the focus back. So on Windows a single
            // click waits a moment, and any click or double click after it cancels it.
            static LAST_DOUBLE: Mutex<Option<Instant>> = Mutex::new(None);
            static CLICK_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let double_click = || {
                *LAST_DOUBLE.lock().unwrap() = Some(Instant::now());
                CLICK_SEQ.fetch_add(1, Ordering::SeqCst);
                let app = tray.app_handle().clone();
                std::thread::spawn(move || {
                    close_window(&app, "popover");
                    open_settings(&app);
                });
            };
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = ev {
                double_click();
                return;
            }
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = ev {
                if cfg!(target_os = "linux") {
                    return;
                }
                if LAST_DOUBLE.lock().unwrap().map_or(false, |t| t.elapsed() < Duration::from_millis(400)) {
                    return;
                }
                // macOS never reports tray double clicks, so two quick clicks in a row count as one.
                if cfg!(target_os = "macos") {
                    static LAST_UP: Mutex<Option<Instant>> = Mutex::new(None);
                    let window = Duration::from_millis(platform::double_click_ms());
                    let mut last = LAST_UP.lock().unwrap();
                    if last.map_or(false, |t| t.elapsed() < window) {
                        *last = None;
                        drop(last);
                        double_click();
                        return;
                    }
                    *last = Some(Instant::now());
                }
                let seq = CLICK_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
                let p = rect.position.to_physical::<f64>(1.0);
                let s = rect.size.to_physical::<f64>(1.0);
                let app = tray.app_handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(platform::double_click_ms().min(350)));
                    if CLICK_SEQ.load(Ordering::SeqCst) == seq {
                        toggle_popover(&app, Some((p.x, p.y, s.width, s.height)));
                    }
                });
            }
        })
        .on_menu_event(|app, ev| {
            let id = ev.id().as_ref();
            match id {
                "pause_30" => run_action(app, "pause", Some(30)),
                "pause_60" => run_action(app, "pause", Some(60)),
                "pause_120" => run_action(app, "pause", Some(120)),
                "pause_forever" => run_action(app, "pause", None),
                "show_status" => {
                    let app = app.clone();
                    std::thread::spawn(move || show_popover(&app, None));
                }
                "update" => {
                    let url = state(app).update.lock().unwrap().as_ref().map(|u| u.url.clone());
                    match url {
                        Some(u) => platform::open_url(&u),
                        None => open_settings(app),
                    }
                }
                other => run_action(app, other, None),
            }
        })
        .build(app)?;

    let mut labeled = vec![take.clone(), skip.clone(), p30, p60, p120, ptomorrow, pforever, resume.clone(), settings_item.clone(), quit.clone()];
    labeled.push(show_status.clone());
    *state(app).tray.lock().unwrap() = Some(TrayItems {
        menu,
        status,
        skip: skip.0,
        focus: focus.0,
        evening: evening.0,
        pause,
        resume: resume.0,
        update: None,
        labeled,
    });
    Ok(())
}

/// Re-labels the tray menu after the language setting changes.
fn relabel_tray(app: &AppHandle) {
    let lang = ui_lang(app);
    {
        let st = state(app);
        let guard = st.tray.lock().unwrap();
        let Some(t) = guard.as_ref() else { return };
        for (item, key) in &t.labeled {
            let _ = item.set_text(i18n::tr(lang, key, &[]));
        }
        let _ = t.pause.set_text(i18n::tr(lang, "Pause", &[]));
    }
    let active = state(app).evening.lock().unwrap().active();
    relabel_evening(app, active, gamma::support().is_ok());
    let update = state(app).update.lock().unwrap().clone();
    set_tray_update(app, update.as_ref());
}

// ==================================================================== updates

/// Shows (or clears) the "Update available" entry at the top of the tray menu.
fn set_tray_update(app: &AppHandle, info: Option<&UpdateInfo>) {
    let lang = ui_lang(app); // read before taking the tray lock
    let st = state(app);
    let mut guard = st.tray.lock().unwrap();
    let Some(t) = guard.as_mut() else { return };
    if let Some((item, sep)) = t.update.take() {
        let _ = t.menu.remove(&item);
        let _ = t.menu.remove(&sep);
    }
    if let Some(u) = info {
        let label = i18n::tr(lang, "Update available: v{v}…", &[("v", &u.version)]);
        if let (Ok(item), Ok(sep)) = (
            MenuItem::with_id(app, "update", label, true, None::<&str>),
            PredefinedMenuItem::separator(app),
        ) {
            let _ = t.menu.insert(&item, 0);
            let _ = t.menu.insert(&sep, 1);
            t.update = Some((item, sep));
        }
    }
}

/// Asks GitHub for a newer release. `manual` checks ignore a skipped version.
fn check_for_updates(app: &AppHandle, manual: bool) -> Result<Option<UpdateInfo>, String> {
    let current = app.package_info().version.to_string();
    let found = match updates::fetch_latest(&current) {
        Ok(f) => f,
        Err(e) => {
            log_warn!("update check failed ({}): {e}", if manual { "manual" } else { "automatic" });
            return Err(e);
        }
    };
    match &found {
        Some(u) => log_info!("update check: v{} is available (running {current})", u.version),
        None => log_info!("update check: up to date ({current})"),
    }
    let skipped = state(app).core.lock().unwrap().settings.skipped_update.clone();
    let visible = found.filter(|u| manual || u.version != skipped);
    let changed = {
        let st = state(app);
        let mut slot = st.update.lock().unwrap();
        let changed = *slot != visible;
        *slot = visible.clone();
        changed
    };
    if changed {
        set_tray_update(app, visible.as_ref());
        let _ = app.emit("update-status", &visible);
    }
    Ok(visible)
}

fn spawn_update_checker(app: AppHandle) {
    std::thread::spawn(move || {
        // Give startup a moment, then check every 6 hours (when enabled).
        std::thread::sleep(Duration::from_secs(20));
        loop {
            let enabled = state(&app).core.lock().unwrap().settings.check_updates;
            if enabled {
                let _ = check_for_updates(&app, false);
            }
            std::thread::sleep(Duration::from_secs(6 * 60 * 60));
        }
    });
}

#[tauri::command]
async fn check_updates_now(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    check_for_updates(&app, true)
}

#[tauri::command]
async fn get_update(app: AppHandle) -> Option<UpdateInfo> {
    state(&app).update.lock().unwrap().clone()
}

#[tauri::command]
async fn skip_update(app: AppHandle, version: String) {
    {
        let st = state(&app);
        let mut core = st.core.lock().unwrap();
        core.settings.skipped_update = version;
        core.settings.save(&st.paths.settings);
    }
    *state(&app).update.lock().unwrap() = None;
    set_tray_update(&app, None);
    let _ = app.emit("update-status", Option::<UpdateInfo>::None);
}

// ------------------------------------------------------ screen recording permission

/// Where the settings window was when Yonder asked for Screen Recording permission.
/// macOS then offers to "Quit & Reopen" Yonder, and the window comes back here.
#[derive(serde::Serialize, serde::Deserialize)]
struct Resume {
    page: String,
    step: u8,
    at: i64,
}

/// Whether breaks may photograph the desktop (always true off macOS).
#[tauri::command]
async fn screen_access() -> bool {
    platform::screen_access()
}

#[tauri::command]
async fn request_screen_access(app: AppHandle, page: String, step: u8) {
    let st = state(&app);
    if let Some(dir) = st.paths.resume.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let resume = Resume { page, step, at: engine::now_epoch() };
    if let Ok(json) = serde_json::to_string(&resume) {
        if let Err(e) = std::fs::write(&st.paths.resume, json) {
            log_warn!("couldn't remember the settings screen: {e}");
        }
    }
    // macOS shows its own prompt only once, and an earlier break's screenshot may already
    // have used it up. So only the first ask from setup (a fresh install) relies on the
    // prompt; every other ask also opens the Screen Recording pane in System Settings.
    let asked_before = st.paths.screen_asked.exists() || step == 0;
    let _ = std::fs::write(&st.paths.screen_asked, "");
    log_info!("asking for screen recording permission (asked before: {asked_before})");
    platform::request_screen_access(asked_before);
}

/// When this Yonder process started (epoch seconds).
static STARTED_AT: std::sync::OnceLock<i64> = std::sync::OnceLock::new();

/// The screen saved by `request_screen_access`, once, and only when Yonder has restarted
/// since (closing and reopening the settings window alone doesn't count) within 30 minutes.
#[tauri::command]
async fn take_resume(app: AppHandle) -> Option<Resume> {
    let path = &state(&app).paths.resume;
    let text = std::fs::read_to_string(path).ok()?;
    let resume: Resume = serde_json::from_str(&text).ok()?;
    let started = *STARTED_AT.get()?;
    if resume.at >= started {
        return None; // asked in this run: keep it for the relaunch
    }
    let _ = std::fs::remove_file(path);
    (started - resume.at < 30 * 60).then_some(resume)
}

#[tauri::command]
async fn open_logs_folder(app: AppHandle) {
    let dir = state(&app).log_dir.clone();
    let _ = std::fs::create_dir_all(&dir);
    platform::open_folder(&dir);
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

// ================================================================= tick loop

/// Why breaks are held right now: an English text key plus the app, if one is to blame.
#[derive(Clone, PartialEq)]
struct SmartHold {
    key: &'static str,
    app: Option<String>,
}

impl SmartHold {
    fn new(key: &'static str) -> Self {
        SmartHold { key, app: None }
    }

    /// The reason as shown to the user, naming the app that holds the break when known:
    /// "you're in a call (Discord)".
    fn text(&self, lang: &str) -> String {
        match &self.app {
            Some(a) if self.key == "{app} is open" => i18n::tr(lang, self.key, &[("app", a)]),
            Some(a) => format!("{} ({a})", i18n::tr(lang, self.key, &[])),
            None => i18n::tr(lang, self.key, &[]),
        }
    }

    /// English description for the log, including the app name.
    fn log_text(&self) -> String {
        self.text("en")
    }
}

/// The reason smart pause is active, and whether it is purely a fullscreen app.
fn smart_reason(s: &Settings, scanner: &mut platform::ProcessScanner, frame: u64, cache: &mut Option<(SmartHold, bool)>) -> Option<(SmartHold, bool)> {
    if !s.smart.enabled {
        return None;
    }
    if frame % 3 != 0 {
        return cache.clone();
    }
    let mut reason = None;
    if s.smart.meetings && platform::mic_or_camera_in_use() {
        reason = Some(SmartHold { key: "you're in a call", app: platform::mic_or_camera_app() });
    }
    let mut fullscreen = false;
    if reason.is_none() && s.smart.fullscreen && platform::fullscreen_app_active(scanner) {
        reason = Some(SmartHold { key: "a fullscreen app is running", app: platform::foreground_app_name(scanner) });
        fullscreen = true;
    }
    // A fullscreen app together with any other reason (DND, sharing, a focus app) keeps holding.
    if fullscreen && ((s.smart.dnd && platform::dnd_active()) || (s.smart.screen_share && platform::screen_shared_by_system())) {
        fullscreen = false;
    }
    if reason.is_none() && s.smart.dnd && platform::dnd_active() {
        reason = Some(SmartHold::new("Do Not Disturb is on"));
    }
    if reason.is_none() && s.smart.screen_share && platform::screen_shared_by_system() {
        reason = Some(SmartHold::new("you're sharing or recording your screen"));
    }
    let need_scan = !s.smart.apps.is_empty()
        || s.smart.screen_share
        || (s.smart.meetings && cfg!(target_os = "macos"));
    if reason.is_none() && need_scan {
        let names = scanner.names();
        let meeting_app = if s.smart.meetings {
            names.iter().find(|n| platform::MEETING_PROCESS_HINTS.iter().any(|h| *n == h))
        } else {
            None
        };
        let capture_app = if s.smart.screen_share { platform::screen_capture_app(&names) } else { None };
        if let Some(n) = meeting_app {
            reason = Some(SmartHold { key: "you're in a call", app: Some(n.clone()) });
        } else if let Some(app) = capture_app {
            reason = Some(SmartHold { key: "you're sharing or recording your screen", app: Some(app.to_string()) });
        } else if let Some(app) = s.smart.apps.iter().find(|a| {
            let a = a.trim().to_lowercase();
            let a = a.trim_end_matches(".exe").trim_end_matches(".app");
            !a.is_empty() && names.iter().any(|n| n == a || n.starts_with(a))
        }) {
            reason = Some(SmartHold { key: "{app} is open", app: Some(app.trim().to_string()) });
        }
    }
    let reason = reason.map(|r| (r, fullscreen));
    *cache = reason.clone();
    reason
}

fn save_state(app: &AppHandle) {
    let st = state(app);
    let saved = st.core.lock().unwrap().saved_state();
    saved.save(&st.paths.state);
}

fn spawn_ticker(app: AppHandle) {
    std::thread::spawn(move || {
        let mut scanner = platform::ProcessScanner::new();
        let mut cache: Option<(SmartHold, bool)> = None;
        let mut last_hold: Option<(SmartHold, bool)> = None;
        let mut frame: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            frame += 1;
            let idle = platform::idle_secs();
            let settings = state(&app).core.lock().unwrap().settings.clone();
            let on_break = state(&app).core.lock().unwrap().e.brk.is_some();
            let hold = if on_break {
                cache.clone()
            } else {
                smart_reason(&settings, &mut scanner, frame, &mut cache)
            };
            if hold != last_hold {
                match &hold {
                    Some((h, _)) => log_info!("smart pause on: {}", h.log_text()),
                    None => log_info!("smart pause off"),
                }
                last_hold = hold.clone();
            }
            evening_tick(&app);
            let lang = i18n::resolve(&settings.language);
            let fullscreen_only = hold.as_ref().map_or(false, |h| h.1);
            let reason = hold.map(|h| h.0.text(lang));
            with_core(&app, |c| {
                c.e.smart_fullscreen = fullscreen_only;
                c.tick(idle, reason)
            });
            if on_break && frame % 2 == 0 {
                sync_break_windows(&app);
            }
            if frame % 60 == 0 {
                save_state(&app);
            }
        }
    });
}

// =================================================================== evening

/// What the evening filter reports to the settings window.
#[derive(serde::Serialize)]
struct EveningStatus {
    /// The display can be filtered here.
    supported: bool,
    /// Why not: "wayland", "no-display" or "unsupported".
    reason: String,
    /// The filter is on (or fading) right now.
    active: bool,
    /// How strong it is right now, 0..=1.
    strength: f64,
}

#[tauri::command]
async fn get_evening_status(app: AppHandle) -> EveningStatus {
    let (active, strength) = {
        let st = state(&app);
        let c = st.evening.lock().unwrap();
        (c.active(), c.strength())
    };
    match gamma::support() {
        Ok(()) => EveningStatus { supported: true, reason: String::new(), active, strength },
        Err(r) => EveningStatus { supported: false, reason: r.into(), active: false, strength: 0.0 },
    }
}

/// Runs once a second (and right after the tray toggle or Preview): works out what the
/// display should look like and changes it only when that differs from what is applied.
fn evening_tick(app: &AppHandle) {
    use chrono::Timelike;
    let cfg = state(app).core.lock().unwrap().settings.evening.clone();
    let now = chrono::Local::now();
    let now_min = (now.hour() * 60 + now.minute()) as f64 + now.second() as f64 / 60.0;
    let supported = gamma::support().is_ok();
    let st = state(app);
    let mut c = st.evening.lock().unwrap();
    // Without a way to filter the display the controller stays idle.
    let step = if supported {
        c.step(&cfg, now_min, Instant::now(), gamma::MIN_CHANNEL)
    } else {
        evening::Step::Nothing
    };
    match step {
        evening::Step::Apply(m) => match gamma::apply(m) {
            Ok(()) => {
                // Leave a marker first: if Yonder dies while the filter is on, the next start cleans up.
                let _ = std::fs::write(&st.paths.evening_marker, b"1");
                log_debug!("evening filter {:.0}%", c.strength() * 100.0);
            }
            Err(e) => {
                c.apply_failed();
                static WARNED: AtomicBool = AtomicBool::new(false);
                if !WARNED.swap(true, Ordering::Relaxed) {
                    log_warn!("evening filter couldn't be applied: {e}");
                }
            }
        },
        evening::Step::Reset => {
            if let Err(e) = gamma::reset() {
                log_warn!("evening filter couldn't be removed: {e}");
            }
            let _ = std::fs::remove_file(&st.paths.evening_marker);
            log_info!("evening filter off");
        }
        evening::Step::Nothing => {}
    }
    let active = c.active();
    drop(c);
    // keep the tray label and the quick-toggle state in step
    static LAST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    let want = 1 + u8::from(active) + if supported { 0 } else { 2 };
    if LAST.swap(want, Ordering::Relaxed) != want {
        relabel_evening(app, active, supported);
    }
}

fn relabel_evening(app: &AppHandle, active: bool, supported: bool) {
    let lang = ui_lang(app);
    let key = if active { "Turn off evening filter" } else { "Turn on evening filter" };
    if let Some(t) = state(app).tray.lock().unwrap().as_ref() {
        let _ = t.evening.set_text(i18n::tr(lang, key, &[]));
        let _ = t.evening.set_enabled(supported);
    }
}

/// On start: if the filter was left on (a crash, a kill) or is about to be used, put the
/// display back to normal first so it can never start from a stuck tint.
fn evening_startup(app: &AppHandle) {
    let st = state(app);
    let marker = st.paths.evening_marker.exists();
    let enabled = st.core.lock().unwrap().settings.evening.enabled;
    if (marker || enabled) && gamma::support().is_ok() {
        if marker {
            log_info!("evening filter was left on by a previous run; resetting the display");
        }
        if let Err(e) = gamma::reset() {
            log_warn!("couldn't reset the display on start: {e}");
        }
    }
    let _ = std::fs::remove_file(&st.paths.evening_marker);
}

/// On quit: hand the display back untouched.
fn evening_restore(app: &AppHandle) {
    let st = state(app);
    let was_on = st.evening.lock().unwrap().applied().is_some() || st.paths.evening_marker.exists();
    if was_on {
        if let Err(e) = gamma::reset() {
            log_warn!("couldn't restore the display on quit: {e}");
        }
        st.evening.lock().unwrap().mark_reset();
        let _ = std::fs::remove_file(&st.paths.evening_marker);
        log_info!("evening filter removed on quit");
    }
}

// ======================================================================= run

fn log_startup(app: &AppHandle, s: &Settings, parsed: &cli::Parsed) {
    let os = sysinfo::System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into());
    log_info!(
        "Yonder {} starting on {os} ({}), language {}",
        app.package_info().version,
        std::env::consts::ARCH,
        i18n::resolve(&s.language)
    );
    #[cfg(target_os = "linux")]
    log_info!(
        "session: {} / {}{}",
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()),
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into()),
        if std::env::var_os("APPIMAGE").is_some() { " (AppImage)" } else { "" }
    );
    log_info!(
        "smart pause {} (meetings {}, fullscreen {}, dnd {}, screen share {}, {} apps), break style {}, verbose {}",
        s.smart.enabled,
        s.smart.meetings,
        s.smart.fullscreen,
        s.smart.dnd,
        s.smart.screen_share,
        s.smart.apps.len(),
        s.break_style,
        logger::verbose()
    );
    if parsed.background {
        log_debug!("started with --background");
    }
}

/// Registers the yonder:// scheme where the installer can't, and listens for OS link events.
fn setup_deep_links(app: &AppHandle) {
    let links = app.deep_link();
    // AppImages aren't installed, so there is no package to register the scheme.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        match links.register("yonder") {
            Ok(()) => log_debug!("registered the yonder:// scheme for this AppImage"),
            Err(e) => log_warn!("couldn't register the yonder:// scheme: {e}"),
        }
    }
    let handle = app.clone();
    links.on_open_url(move |ev| {
        run_links(&handle, ev.urls().iter().map(|u| u.to_string()).collect());
    });
    // macOS delivers the link that launched the app as an event; others pass it as an argument.
    #[cfg(target_os = "macos")]
    match links.get_current() {
        Ok(Some(urls)) => run_links(app, urls.iter().map(|u| u.to_string()).collect()),
        Ok(None) => {}
        Err(e) => log_warn!("couldn't read the launch link: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let parsed = cli::parse_args(args.get(1..).unwrap_or(&[]));
            if parsed.verbose {
                logger::set_verbose(true);
            }
            if parsed.is_request() {
                run_commands(app, "command-line", &parsed);
            } else {
                log_debug!("second launch without commands: opening settings");
                open_settings(app);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .setup(|app| {
            let _ = STARTED_AT.set(engine::now_epoch());
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config = app.path().app_config_dir()?;
            let data = app.path().app_data_dir()?;
            let paths = Paths {
                settings: config.join("settings.json"),
                stats: data.join("stats.json"),
                state: data.join("state.json"),
                background: data.join("background.txt"),
                sounds: data.join("sounds"),
                resume: data.join("resume.json"),
                screen_asked: data.join("screen-asked"),
                evening_marker: data.join("evening-active"),
            };
            let log_dir = app.path().app_log_dir()?;
            logger::init(log_dir.clone());
            logger::install_panic_hook();
            let args: Vec<String> = std::env::args().collect();
            let parsed = cli::parse_args(args.get(1..).unwrap_or(&[]));
            let settings = Settings::load(&paths.settings);
            logger::set_verbose(parsed.verbose || settings.verbose_logging);
            log_startup(app.handle(), &settings, &parsed);
            let stats = Stats::load(&paths.stats);
            let first_run = !settings.onboarded;
            let launch = settings.launch_at_login;
            let mut core = Core::new(settings, stats);
            if let Some(saved) = engine::SavedState::load(&paths.state) {
                core.restore(&saved, engine::now_epoch());
            }
            app.manage(AppState {
                core: Mutex::new(core),
                paths,
                evening: Mutex::new(evening::Controller::default()),
                snapshots: Mutex::new(HashMap::new()),
                break_wins: Mutex::new(BreakWins::default()),
                reminder_queue: Mutex::new(Vec::new()),
                tray: Mutex::new(None),
                tray_look: Mutex::new(None),
                update: Mutex::new(None),
                popover_anchor: Mutex::new(None),
                popover_closed: Mutex::new(None),
                last_link: Mutex::new(None),
                log_dir,
                before_focus: Mutex::new(None),
            });

            let handle = app.handle().clone();
            if let Err(e) = build_tray(&handle) {
                log_warn!("couldn't create the tray icon: {e}");
                return Err(e.into());
            }
            evening_startup(&handle);
            register_shortcuts(&handle);
            if first_run && launch {
                if let Err(e) = handle.autolaunch().enable() {
                    log_warn!("couldn't enable launch at login: {e}");
                }
            }
            // Launched with a command (or `--background`): stay quiet unless it's the first run.
            if first_run || !parsed.is_request() {
                open_settings(&handle);
            }
            setup_deep_links(&handle);
            spawn_update_checker(handle.clone());
            spawn_ticker(handle.clone());
            run_commands(&handle, "command-line", &parsed);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_state,
            get_stats,
            reset_stats,
            action,
            get_screenshot,
            export_settings,
            check_backup,
            import_settings,
            save_background,
            get_background,
            add_custom_sound,
            get_custom_sound,
            remove_custom_sound,
            take_reminders,
            get_evening_status,
            fit_window,
            open_url,
            platform_name,
            check_updates_now,
            get_update,
            skip_update,
            app_version,
            open_logs_folder,
            screen_access,
            request_screen_access,
            take_resume
        ])
        .build(tauri::generate_context!())
        .expect("error while building Yonder")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                evening_restore(app);
                save_state(app);
            }
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                // Closing the last window keeps Yonder running in the tray.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::menu_status;

    #[test]
    fn tray_menu_shows_the_first_sentence() {
        assert_eq!(
            menu_status("Break due — waiting while you're in a call (Discord). It starts 1 minute after that. At the latest in 59 minutes."),
            "Break due — waiting while you're in a call (Discord)"
        );
        assert_eq!(menu_status("Next break in 12:34"), "Next break in 12:34");
        assert_eq!(menu_status("休息时间已到 — 正在等待：你正在通话。结束后 1 分钟 开始。"), "休息时间已到 — 正在等待：你正在通话");
    }
}
