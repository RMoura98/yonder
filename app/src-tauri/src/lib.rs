mod engine;
mod i18n;
mod media;
mod platform;
mod settings;
mod stats;
mod updates;

use engine::{Core, Effect, Snapshot};
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
}

struct TrayItems {
    menu: Menu<Wry>,
    status: MenuItem<Wry>,
    skip: MenuItem<Wry>,
    /// "Start/Stop focus session"; the label follows the session state.
    focus: MenuItem<Wry>,
    pause: Submenu<Wry>,
    resume: MenuItem<Wry>,
    /// "Update available…" entry, inserted at the top only when there is one.
    update: Option<(MenuItem<Wry>, PredefinedMenuItem<Wry>)>,
    /// Static items and their English source text, for re-labelling.
    labeled: Vec<(MenuItem<Wry>, &'static str)>,
}

struct AppState {
    core: Mutex<Core>,
    paths: Paths,
    snapshots: Mutex<HashMap<String, String>>,
    reminder_queue: Mutex<Vec<Reminder>>,
    tray: Mutex<Option<TrayItems>>,
    /// (light taskbar, paused) the tray glyph was last drawn for
    tray_look: Mutex<Option<(bool, bool)>>,
    /// Newer release found by the update checker (and not skipped).
    update: Mutex<Option<UpdateInfo>>,
    /// Where the tray icon is (physical x, y, w, h), or the cursor, for placing the popover.
    popover_anchor: Mutex<Option<(f64, f64, f64, f64)>>,
    /// When the popover last closed, so the tray click that blurred it doesn't reopen it.
    popover_closed: Mutex<Option<Instant>>,
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

// ================================================================== windows

fn open_settings(app: &AppHandle) {
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

fn cursor_monitor(app: &AppHandle) -> Option<tauri::Monitor> {
    let pos = app.cursor_position().ok().map(|p| (p.x, p.y));
    monitor_at(app, pos)
}

/// The monitor containing a physical point (the primary one if it is off every screen).
fn monitor_at(app: &AppHandle, pos: Option<(f64, f64)>) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().ok()?;
    if let Some((px, py)) = pos {
        for m in &monitors {
            let (mp, ms) = (m.position(), m.size());
            if px >= mp.x as f64
                && py >= mp.y as f64
                && px < (mp.x + ms.width as i32) as f64
                && py < (mp.y + ms.height as i32) as f64
            {
                return Some(m.clone());
            }
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
    let _ = window.show();
}

/// Places a floating window horizontally centered (or right-aligned) near the top.
fn place_top(app: &AppHandle, win: &WebviewWindow, align_right: bool) {
    let Some(m) = cursor_monitor(app) else { return };
    let scale = m.scale_factor();
    let size = win.outer_size().unwrap_or(PhysicalSize::new(400, 200));
    let mp = m.position();
    let ms = m.size();
    let top = mp.y + (48.0 * scale) as i32;
    let x = if align_right {
        mp.x + ms.width as i32 - size.width as i32 - (20.0 * scale) as i32
    } else {
        mp.x + (ms.width as i32 - size.width as i32) / 2
    };
    let _ = win.set_position(PhysicalPosition::new(x, top));
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

/// A tiny "Starting break in N" pill that trails the mouse for the last seconds.
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
    // The click that blurred (and so closed) the popover must not reopen it.
    let recent = state(app).popover_closed.lock().unwrap().map_or(false, |t| t.elapsed() < Duration::from_millis(350));
    if !recent {
        show_popover(app, anchor);
    }
}

fn show_popover(app: &AppHandle, anchor: Option<(f64, f64, f64, f64)>) {
    if let Some(w) = app.get_webview_window("popover") {
        let _ = w.set_focus();
        return;
    }
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
        tauri::WindowEvent::Destroyed => *state(&handle).popover_closed.lock().unwrap() = Some(Instant::now()),
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
    let size = win.outer_size().unwrap_or(PhysicalSize::new(332, 400));
    let (mp, ms) = (m.position(), m.size());
    let (w, h) = (size.width as f64, size.height as f64);
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

/// "Notification only" style: no window at all, just a native notification plus the
/// break sound (played by a tiny hidden page, since sounds are Web Audio).
fn notify_break(app: &AppHandle) {
    let (lang, brk, sound) = {
        let st = state(app);
        let c = st.core.lock().unwrap();
        (i18n::resolve(&c.settings.language), c.e.brk.clone(), c.settings.sound_enabled)
    };
    if let Some(b) = brk {
        let body = if b.kind == "short" && b.total_secs < 60 {
            i18n::tr(lang, "Time for a {n}-second break — look at something far away", &[("n", &b.total_secs.to_string())])
        } else {
            let n = ((b.total_secs + 30) / 60).max(1).to_string();
            if b.kind == "long" {
                i18n::tr(lang, "Time for a {n}-minute break — stand up, stretch, refill your water", &[("n", &n)])
            } else {
                i18n::tr(lang, "Time for a {n}-minute break — look at something far away", &[("n", &n)])
            }
        };
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
    if state(app).core.lock().unwrap().settings.break_style == "notify" {
        return notify_break(app);
    }
    let Ok(monitors) = app.available_monitors() else { return };
    let needs_shot = {
        let st = state(app);
        let s = &st.core.lock().unwrap().settings;
        s.break_style == "split" || (s.break_style != "gentle" && s.background == "blur")
    };
    let gentle = state(app).core.lock().unwrap().settings.break_style == "gentle";
    let primary = app.primary_monitor().ok().flatten();

    if needs_shot {
        // Make sure the heads-up toast is gone before we photograph the desktop.
        if app.get_webview_window("prebreak").is_some() {
            close_window(app, "prebreak");
            std::thread::sleep(Duration::from_millis(150));
        }
        let st = state(app);
        let mut shots = st.snapshots.lock().unwrap();
        shots.clear();
        for (i, m) in monitors.iter().enumerate() {
            if let Some(url) = platform::screenshot_near(m.position().x, m.position().y) {
                shots.insert(format!("break-{i}"), url);
            }
        }
    }

    for (i, m) in monitors.iter().enumerate() {
        let label = format!("break-{i}");
        if app.get_webview_window(&label).is_some() {
            continue;
        }
        let is_primary = primary
            .as_ref()
            .map(|p| p.position() == m.position())
            .unwrap_or(i == 0);
        let url = format!("break.html?primary={}", if is_primary { 1 } else { 0 });
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
            continue;
        };
        let _ = w.set_position(*m.position());
        let _ = w.set_size(*m.size());
        // Native fullscreen covers the monitor pixel-exactly (no stray borders), which
        // keeps the split-screen illusion seamless. macOS fullscreen would open a new
        // Space with an animation, so there we keep the borderless, monitor-sized window.
        // Gentle mode is a click-through veil: no fullscreen (keeps the taskbar and the
        // user's apps undisturbed), never focused, and the mouse passes straight through.
        if gentle {
            let _ = w.set_ignore_cursor_events(true);
            show_quietly(&w);
            continue;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = w.set_fullscreen(true);
        let _ = w.show();
        if is_primary {
            let _ = w.set_focus();
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
            Effect::ShowPrebreak => show_prebreak(app),
            Effect::HidePrebreak => {
                close_window(app, "prebreak");
                close_window(app, "cursor");
            }
            Effect::StartBreak => {
                close_window(app, "cursor");
                if state(app).core.lock().unwrap().settings.pause_media {
                    media::pause_playing();
                }
                start_break_windows(app)
            }
            Effect::EndBreak { delay_ms } => {
                // Resume only what we paused, even if the setting was turned off mid-break.
                media::resume_paused();
                end_break_windows(app, delay_ms)
            }
            Effect::ShowReminders(list) => show_reminders(app, list),
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

fn update_tray(app: &AppHandle, snap: &Snapshot) {
    let lang = ui_lang(app);
    let st = state(app);
    let guard = st.tray.lock().unwrap();
    let Some(t) = guard.as_ref() else { return };
    let _ = t.status.set_text(&snap.label);
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
        // Re-tint only when the taskbar theme or the paused state changes.
        let look = (platform::taskbar_is_light(), paused);
        let mut last = st.tray_look.lock().unwrap();
        if *last != Some(look) {
            *last = Some(look);
            let _ = tray.set_icon(Some(tray_icon(look.0, look.1)));
            let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
        }
        drop(last);
        let _ = tray.set_tooltip(Some(format!("Yonder — {}", snap.label)));
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
        let _ = gs.on_shortcut(accel.trim(), move |app, _s, ev| {
            if ev.state == ShortcutState::Pressed {
                run_action(app, action, None);
            }
        });
    };
    bind(&sc.take_break, "take_break");
    bind(&sc.toggle_pause, "toggle_pause");
    bind(&sc.skip_break, "skip");
    bind(&sc.toggle_focus, "toggle_focus");
}

// =================================================================== actions

fn run_action(app: &AppHandle, name: &str, arg: Option<i64>) {
    match name {
        "take_break" => with_core(app, |c| c.take_break_now(arg.map(|a| a as u32))),
        "skip" => with_core(app, |c| c.skip_next()),
        "skip_held" => with_core(app, |c| c.skip_held()),
        "end_break" => with_core(app, |c| c.skip_or_end_break()),
        "snooze" => with_core(app, |c| c.snooze(arg.unwrap_or(5) as u32)),
        "start_focus" => with_core(app, |c| {
            c.start_focus();
            vec![]
        }),
        "stop_focus" => with_core(app, |c| c.stop_focus()),
        "toggle_focus" => with_core(app, |c| c.toggle_focus()),
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
        "test_prebreak" => show_prebreak(app),
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
        let _ = if launch { al.enable() } else { al.disable() };
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
    items.extend([&take.0 as &dyn IsMenuItem<Wry>, &skip.0, &focus.0, &pause, &resume.0, &sep2, settings, quit_item]);
    let menu = Menu::with_items(app, &items)?;

    TrayIconBuilder::with_id("main")
        .icon(tray_icon(platform::taskbar_is_light(), false))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Yonder")
        .menu(&menu)
        // Left click opens the popover (right click keeps the menu); Linux hosts keep the menu.
        .show_menu_on_left_click(linux)
        .on_tray_icon_event(|tray, ev| {
            // A double click (Windows) opens the settings window. Windows reports it as
            // click, double click, click, so the clicks around it must not toggle the popover.
            static LAST_DOUBLE: Mutex<Option<Instant>> = Mutex::new(None);
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = ev {
                *LAST_DOUBLE.lock().unwrap() = Some(Instant::now());
                let app = tray.app_handle().clone();
                std::thread::spawn(move || {
                    close_window(&app, "popover");
                    open_settings(&app);
                });
                return;
            }
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = ev {
                if cfg!(target_os = "linux") {
                    return;
                }
                if LAST_DOUBLE.lock().unwrap().map_or(false, |t| t.elapsed() < Duration::from_millis(400)) {
                    return;
                }
                let p = rect.position.to_physical::<f64>(1.0);
                let s = rect.size.to_physical::<f64>(1.0);
                let app = tray.app_handle().clone();
                std::thread::spawn(move || toggle_popover(&app, Some((p.x, p.y, s.width, s.height))));
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
    let found = updates::fetch_latest(&current)?;
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

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

// ================================================================= tick loop

fn smart_reason(s: &Settings, scanner: &mut platform::ProcessScanner, frame: u64, cache: &mut Option<String>) -> Option<String> {
    let lang = i18n::resolve(&s.language);
    if !s.smart.enabled {
        return None;
    }
    if frame % 3 != 0 {
        return cache.clone();
    }
    let mut reason = None;
    if s.smart.meetings && platform::mic_or_camera_in_use() {
        reason = Some(i18n::tr(lang, "you're in a call", &[]));
    }
    if reason.is_none() && s.smart.fullscreen && platform::fullscreen_app_active(scanner) {
        reason = Some(i18n::tr(lang, "a fullscreen app is running", &[]));
    }
    if reason.is_none() && s.smart.dnd && platform::dnd_active() {
        reason = Some(i18n::tr(lang, "Do Not Disturb is on", &[]));
    }
    if reason.is_none() && s.smart.screen_share && platform::screen_shared_by_system() {
        reason = Some(i18n::tr(lang, "you're sharing or recording your screen", &[]));
    }
    let need_scan = !s.smart.apps.is_empty()
        || s.smart.screen_share
        || (s.smart.meetings && cfg!(target_os = "macos"));
    if reason.is_none() && need_scan {
        let names = scanner.names();
        if s.smart.meetings
            && names
                .iter()
                .any(|n| platform::MEETING_PROCESS_HINTS.iter().any(|h| n == h))
        {
            reason = Some(i18n::tr(lang, "you're in a call", &[]));
        } else if s.smart.screen_share && platform::screen_capture_app(&names).is_some() {
            reason = Some(i18n::tr(lang, "you're sharing or recording your screen", &[]));
        } else if let Some(app) = s.smart.apps.iter().find(|a| {
            let a = a.trim().to_lowercase();
            let a = a.trim_end_matches(".exe").trim_end_matches(".app");
            !a.is_empty() && names.iter().any(|n| n == a || n.starts_with(a))
        }) {
            reason = Some(i18n::tr(lang, "{app} is open", &[("app", app.trim())]));
        }
    }
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
        let mut cache = None;
        let mut frame: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            frame += 1;
            let idle = platform::idle_secs();
            let settings = state(&app).core.lock().unwrap().settings.clone();
            let on_break = state(&app).core.lock().unwrap().e.brk.is_some();
            let reason = if on_break {
                cache.clone()
            } else {
                smart_reason(&settings, &mut scanner, frame, &mut cache)
            };
            with_core(&app, |c| c.tick(idle, reason));
            if frame % 60 == 0 {
                save_state(&app);
            }
        }
    });
}

// ======================================================================= run

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| open_settings(app)))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .setup(|app| {
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
            };
            let settings = Settings::load(&paths.settings);
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
                snapshots: Mutex::new(HashMap::new()),
                reminder_queue: Mutex::new(Vec::new()),
                tray: Mutex::new(None),
                tray_look: Mutex::new(None),
                update: Mutex::new(None),
                popover_anchor: Mutex::new(None),
                popover_closed: Mutex::new(None),
            });

            let handle = app.handle().clone();
            build_tray(&handle)?;
            register_shortcuts(&handle);
            if first_run && launch {
                let _ = handle.autolaunch().enable();
            }
            let background = std::env::args().any(|a| a == "--background");
            if first_run || !background {
                open_settings(&handle);
            }
            spawn_update_checker(handle.clone());
            spawn_ticker(handle);
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
            fit_window,
            open_url,
            platform_name,
            check_updates_now,
            get_update,
            skip_update,
            app_version
        ])
        .build(tauri::generate_context!())
        .expect("error while building Yonder")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
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
