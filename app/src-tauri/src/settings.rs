use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Reminder {
    pub id: String,
    pub enabled: bool,
    pub title: String,
    pub body: String,
    pub icon: String,
    pub interval_mins: u32,
    /// small | medium | large
    pub size: String,
    /// sound id or "none"
    pub sound: String,
    /// stay on screen until ticked off
    pub sticky: bool,
}

impl Default for Reminder {
    fn default() -> Self {
        Self {
            id: "custom".into(),
            enabled: true,
            title: "Reminder".into(),
            body: String::new(),
            icon: "water_drop".into(),
            interval_mins: 45,
            size: "medium".into(),
            sound: "drop".into(),
            sticky: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Wellness {
    pub posture: Reminder,
    pub blink: Reminder,
    pub custom: Vec<Reminder>,
    pub combine: bool,
    pub reset_after_break: bool,
    pub ripple: bool,
}

impl Default for Wellness {
    fn default() -> Self {
        Self {
            posture: Reminder {
                id: "posture".into(),
                enabled: true,
                title: "Sit tall".into(),
                body: "Shoulders back, feet flat, screen at eye level.".into(),
                icon: "accessibility_new".into(),
                interval_mins: 30,
                size: "medium".into(),
                sound: "soft".into(),
                sticky: false,
            },
            blink: Reminder {
                id: "blink".into(),
                enabled: true,
                title: "Blink slowly".into(),
                body: "Close your eyes for a moment and blink a few times.".into(),
                icon: "visibility".into(),
                interval_mins: 15,
                size: "small".into(),
                sound: "none".into(),
                sticky: false,
            },
            custom: vec![],
            combine: true,
            reset_after_break: false,
            ripple: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SmartPause {
    pub enabled: bool,
    pub meetings: bool,
    pub fullscreen: bool,
    /// What happens when a break comes due while a fullscreen app is in front:
    /// wait (hold it until the app ends) | notify (a quiet notification, no break
    /// window) | show (the break goes ahead anyway) | skip (drop this break).
    pub fullscreen_action: String,
    /// Hold breaks while the system's Do Not Disturb / Focus mode is on.
    pub dnd: bool,
    /// Hold breaks while sharing or recording the screen.
    pub screen_share: bool,
    pub apps: Vec<String>,
    pub cooldown_secs: u32,
    /// Longest a due break may be held by smart pause, in minutes. 0 = no limit.
    pub max_hold_mins: u32,
}

impl Default for SmartPause {
    fn default() -> Self {
        Self {
            enabled: true,
            meetings: true,
            fullscreen: true,
            fullscreen_action: "wait".into(),
            dnd: true,
            screen_share: true,
            apps: vec![],
            cooldown_secs: 60,
            max_hold_mins: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct OfficeHours {
    pub enabled: bool,
    /// Monday..Sunday
    pub days: Vec<bool>,
    pub start: String,
    pub end: String,
}

impl Default for OfficeHours {
    fn default() -> Self {
        Self {
            enabled: false,
            days: vec![true, true, true, true, true, false, false],
            start: "09:00".into(),
            end: "18:00".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct PlannedBreak {
    pub id: String,
    pub enabled: bool,
    pub title: String,
    pub icon: String,
    pub time: String,
    pub duration_mins: u32,
    pub days: Vec<bool>,
}

impl Default for PlannedBreak {
    fn default() -> Self {
        Self {
            id: "lunch".into(),
            enabled: true,
            title: "Lunch".into(),
            icon: "restaurant".into(),
            time: "12:30".into(),
            duration_mins: 30,
            days: vec![true, true, true, true, true, false, false],
        }
    }
}

/// Pomodoro-style focus session that runs alongside the eye breaks.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Focus {
    /// Minutes of screen time between focus breaks.
    pub work_mins: u32,
    pub break_mins: u32,
    pub long_break_mins: u32,
    /// A session is this many focus periods; the last one ends in the long break.
    pub long_every: u32,
    /// An eye break due within this many minutes of a focus break is folded into it.
    pub merge_window_mins: u32,
}

impl Default for Focus {
    fn default() -> Self {
        Self { work_mins: 25, break_mins: 5, long_break_mins: 15, long_every: 4, merge_window_mins: 3 }
    }
}

/// Global hotkeys. All empty by default: a global hotkey takes its keys from
/// every other app, so the person picks their own in Keyboard Shortcuts.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Shortcuts {
    pub take_break: String,
    pub toggle_pause: String,
    pub skip_break: String,
    /// Starts or stops a focus session.
    pub toggle_focus: String,
}

/// Optional evening screen filter: warms and dims the display on a schedule.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Evening {
    /// Run the schedule. Off by default; the tray toggle works either way.
    pub enabled: bool,
    /// Start of the evening, "HH:MM".
    pub from: String,
    /// End of the evening (may be after midnight), "HH:MM".
    pub to: String,
    /// Minutes the filter takes to fade in after `from` and out after `to`.
    pub transition_mins: u32,
    /// Colour temperature at full strength, in kelvin (1900..=6500; 6500 = no tint).
    pub temperature: u32,
    /// Extra dimming at full strength, in percent (0..=60).
    pub dim: u32,
}

impl Default for Evening {
    fn default() -> Self {
        Self { enabled: false, from: "21:00".into(), to: "07:00".into(), transition_mins: 30, temperature: 3400, dim: 10 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Hooks {
    pub on_break_start: String,
    pub on_break_end: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// balanced | deep_focus | eye_care | wellness | custom
    pub mode: String,
    pub work_mins: u32,
    pub break_secs: u32,
    pub long_enabled: bool,
    pub long_mins: u32,
    pub long_every: u32,

    /// casual | balanced | hardcore
    pub skip_difficulty: String,
    pub wait_while_typing: bool,
    pub allow_end_early: bool,
    pub lock_on_break: bool,
    pub pause_media: bool,
    /// Guided eye and stretch exercises on the break screen (long, planned, focus and custom breaks).
    pub exercises: bool,
    /// Also offer a short eye routine during the short breaks.
    pub exercises_short: bool,

    pub prebreak_enabled: bool,
    pub prebreak_secs: u32,
    pub prebreak_visible_secs: u32,
    /// cursor (a small countdown next to the mouse) | card (heads-up card with snooze buttons)
    pub prebreak_style: String,

    pub messages: Vec<String>,
    /// fullscreen | split | gentle | notify
    pub break_style: String,
    /// aurora | nebula | ripple | gradient | image
    pub background: String,
    pub gradient: String,
    pub has_image: bool,
    pub texture: String,
    pub sound_enabled: bool,
    pub sound: String,
    pub volume: f32,
    /// Force Touch trackpad taps during breaks (macOS only; ignored elsewhere).
    pub haptics: bool,

    pub idle_reset_mins: u32,
    pub smart: SmartPause,
    pub office_hours: OfficeHours,
    pub planned: Vec<PlannedBreak>,
    pub wellness: Wellness,
    pub focus: Focus,
    pub shortcuts: Shortcuts,
    /// Set once the old default shortcuts (Ctrl/Cmd+Alt+B/P/S) were cleared. Older
    /// settings files lack it (field-level default: false) and get migrated.
    #[serde(default)]
    pub shortcuts_opt_in: bool,
    pub hooks: Hooks,
    pub evening: Evening,
    pub launch_at_login: bool,
    /// system | light | dark
    pub theme: String,
    pub onboarded: bool,
    /// Ask GitHub for a newer release every few hours.
    pub check_updates: bool,
    /// Write debug-level detail to the log file (also `--verbose`).
    pub verbose_logging: bool,
    /// Show the time until the next break beside the tray icon (macOS menu bar, some Linux panels).
    pub tray_countdown: bool,
    /// Draw the time until the next break into the tray icon (Windows, where text can't sit beside it).
    pub tray_icon_countdown: bool,
    /// A release the user chose to skip (e.g. "1.1.0").
    pub skipped_update: String,
    /// "system" or a language code: en, pt, es, fr, de
    pub language: String,
    /// Audio files the user added; referenced from sound pickers as "file:<id>".
    pub custom_sounds: Vec<CustomSound>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CustomSound {
    pub id: String,
    pub name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: "balanced".into(),
            work_mins: 20,
            break_secs: 20,
            long_enabled: false,
            long_mins: 5,
            long_every: 3,
            skip_difficulty: "balanced".into(),
            wait_while_typing: true,
            allow_end_early: true,
            lock_on_break: false,
            pause_media: false,
            exercises: false,
            exercises_short: false,
            prebreak_enabled: true,
            prebreak_secs: 60,
            prebreak_visible_secs: 10,
            prebreak_style: "cursor".into(),
            messages: vec![
                "Look at something at least 20 feet away.".into(),
                "Let your eyes go soft. Breathe out slowly.".into(),
                "Roll your shoulders back a few times.".into(),
                "Stand up and stretch — your back will thank you.".into(),
                "Sip some water.".into(),
            ],
            break_style: "fullscreen".into(),
            background: "blur".into(),
            gradient: "dusk".into(),
            has_image: false,
            texture: "linen".into(),
            sound_enabled: true,
            sound: "chime".into(),
            volume: 0.6,
            haptics: false,
            idle_reset_mins: 5,
            smart: SmartPause::default(),
            office_hours: OfficeHours::default(),
            planned: vec![],
            wellness: Wellness::default(),
            focus: Focus::default(),
            shortcuts: Shortcuts::default(),
            shortcuts_opt_in: true,
            hooks: Hooks::default(),
            evening: Evening::default(),
            launch_at_login: true,
            theme: "system".into(),
            onboarded: false,
            check_updates: true,
            verbose_logging: false,
            tray_countdown: true,
            tray_icon_countdown: false,
            skipped_update: String::new(),
            custom_sounds: Vec::new(),
            language: "system".into(),
        }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Self {
        let mut s: Settings = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                crate::logger::warn(&format!("settings.json is not valid ({e}); using defaults"));
                Settings::default()
            }),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    crate::logger::warn(&format!("couldn't read settings.json ({e}); using defaults"));
                }
                Settings::default()
            }
        };
        s.migrate();
        s
    }

    /// Older builds stored emoji as icons; icons are now Material Symbol names.
    pub fn migrate(&mut self) {
        let fix = |icon: &mut String, fallback: &str| {
            if icon.is_empty() || !icon.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                *icon = fallback.to_string();
            }
        };
        fix(&mut self.wellness.posture.icon, "accessibility_new");
        fix(&mut self.wellness.blink.icon, "visibility");
        for r in &mut self.wellness.custom {
            fix(&mut r.icon, "notifications");
        }
        for p in &mut self.planned {
            fix(&mut p.icon, "restaurant");
        }
        let e = &mut self.evening;
        e.temperature = e.temperature.clamp(1900, 6500);
        e.dim = e.dim.min(60);
        e.transition_mins = e.transition_mins.min(120);
        let d = Evening::default();
        if crate::evening::parse_hhmm(&e.from).is_none() {
            e.from = d.from;
        }
        if crate::evening::parse_hhmm(&e.to).is_none() {
            e.to = d.to;
        }
        // Shortcuts used to default to Ctrl/Cmd+Alt+B/P/S, global hotkeys that took
        // those keys from every other app. Clear the ones still on the old default
        // once; shortcuts the person picked themselves stay.
        if !self.shortcuts_opt_in {
            let sc = &mut self.shortcuts;
            for (field, old) in [
                (&mut sc.take_break, "CmdOrCtrl+Alt+B"),
                (&mut sc.toggle_pause, "CmdOrCtrl+Alt+P"),
                (&mut sc.skip_break, "CmdOrCtrl+Alt+S"),
            ] {
                if *field == old {
                    field.clear();
                }
            }
            self.shortcuts_opt_in = true;
        }
    }

    pub fn save(&self, path: &PathBuf) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub fn all_reminders(&self) -> Vec<&Reminder> {
        let mut v = vec![&self.wellness.posture, &self.wellness.blink];
        v.extend(self.wellness.custom.iter().take(5));
        v
    }
}

// ------------------------------------------------------------ backup / restore

/// Format version of an exported `.yonder.json` file.
pub const BACKUP_VERSION: u32 = 1;
/// Largest backup file we are willing to parse (it embeds audio and an image).
pub const BACKUP_MAX_BYTES: usize = 80_000_000;

pub fn valid_sound_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct BackupSound {
    pub id: String,
    pub name: String,
    pub data_url: String,
}

/// Everything in a settings backup: the settings plus the user's own files.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Backup {
    /// Format version; 0 means "not a Yonder backup".
    pub yonder_backup: u32,
    pub app_version: String,
    pub settings: Settings,
    /// The custom break background, as a data URL.
    pub background: Option<String>,
    pub sounds: Vec<BackupSound>,
}

/// Parses and sanitises a backup. Errors are English keys translated by the UI.
pub fn parse_backup(text: &str) -> Result<Backup, String> {
    const NOT_BACKUP: &str = "That isn't a Yonder settings file.";
    if text.len() > BACKUP_MAX_BYTES {
        return Err("That backup is too large.".into());
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| NOT_BACKUP.to_string())?;
    let version = value.get("yonder_backup").and_then(|v| v.as_u64()).unwrap_or(0);
    if version == 0 || !value.get("settings").map(|s| s.is_object()).unwrap_or(false) {
        return Err(NOT_BACKUP.into());
    }
    if version > BACKUP_VERSION as u64 {
        return Err("This backup was made by a newer Yonder. Update Yonder and try again.".into());
    }
    let mut b: Backup = serde_json::from_value(value).map_err(|_| NOT_BACKUP.to_string())?;
    b.sanitize();
    Ok(b)
}

impl Backup {
    /// Drops what can't be used and keeps every reference consistent.
    fn sanitize(&mut self) {
        let s = &mut self.settings;
        s.migrate();
        s.wellness.custom.truncate(5);
        // keep only sounds that came with a valid id and real audio
        let mut seen = HashSet::new();
        self.sounds.retain(|c| {
            valid_sound_id(&c.id)
                && (c.data_url.starts_with("data:audio/") || c.data_url.starts_with("data:video/"))
                && seen.insert(c.id.clone())
        });
        let ids: HashSet<&str> = self.sounds.iter().map(|c| c.id.as_str()).collect();
        s.custom_sounds = self
            .sounds
            .iter()
            .map(|c| CustomSound { id: c.id.clone(), name: c.name.trim().chars().take(60).collect() })
            .collect();
        let known = |key: &str| key.strip_prefix("file:").map(|id| ids.contains(id)).unwrap_or(true);
        if !known(&s.sound) {
            s.sound = "chime".into();
        }
        let w = &mut s.wellness;
        for r in std::iter::once(&mut w.posture).chain(std::iter::once(&mut w.blink)).chain(w.custom.iter_mut()) {
            if !known(&r.sound) {
                r.sound = "none".into();
            }
        }
        if !self.background.as_deref().map(|b| b.starts_with("data:image/")).unwrap_or(false) {
            self.background = None;
        }
        s.has_image = self.background.is_some();
        if !s.has_image && s.background == "image" {
            s.background = "blur".into();
        }
        // they already went through setup, and a skipped update means nothing on another machine
        s.onboarded = true;
        s.skipped_update = String::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backup_json(settings_extra: &str, top_extra: &str) -> String {
        format!(r#"{{"yonder_backup":1,"settings":{{"work_mins":33{settings_extra}}}{top_extra}}}"#)
    }

    #[test]
    fn valid_backup_fills_missing_fields_with_defaults() {
        let b = parse_backup(&backup_json("", "")).unwrap();
        assert_eq!(b.settings.work_mins, 33);
        assert_eq!(b.settings.break_secs, Settings::default().break_secs);
        assert!(b.settings.onboarded);
    }

    #[test]
    fn exercises_are_off_for_settings_files_that_predate_them() {
        let b = parse_backup(&backup_json("", "")).unwrap();
        assert!(!b.settings.exercises && !b.settings.exercises_short);
    }

    #[test]
    fn rejects_non_backups_and_newer_versions() {
        assert!(parse_backup("not json").is_err());
        assert!(parse_backup("{}").is_err());
        assert!(parse_backup(r#"{"work_mins":10}"#).is_err());
        assert!(parse_backup(r#"{"yonder_backup":1}"#).is_err());
        assert!(parse_backup(r#"{"yonder_backup":1,"settings":5}"#).is_err());
        assert!(parse_backup(r#"{"yonder_backup":99,"settings":{}}"#).is_err());
        assert!(parse_backup(r#"{"yonder_backup":1,"settings":{"work_mins":"x"}}"#).is_err());
    }

    #[test]
    fn migrates_old_icons_and_caps_custom_reminders() {
        let many = (0..8).map(|i| format!(r#"{{"id":"c{i}","icon":"💧"}}"#)).collect::<Vec<_>>().join(",");
        let b = parse_backup(&backup_json(&format!(r#","wellness":{{"custom":[{many}]}}"#), "")).unwrap();
        assert_eq!(b.settings.wellness.custom.len(), 5);
        assert_eq!(b.settings.wellness.custom[0].icon, "notifications");
    }

    #[test]
    fn sounds_are_validated_and_dangling_references_fall_back() {
        let json = r#"{"yonder_backup":1,
            "settings":{"sound":"file:s1","wellness":{"posture":{"sound":"file:gone"}},
                        "custom_sounds":[{"id":"ghost","name":"x"}]},
            "sounds":[{"id":"s1","name":"Bell","data_url":"data:audio/mpeg;base64,AAAA"},
                      {"id":"../evil","name":"x","data_url":"data:audio/mpeg;base64,AAAA"},
                      {"id":"s2","name":"x","data_url":"data:text/html;base64,AAAA"}]}"#;
        let b = parse_backup(json).unwrap();
        assert_eq!(b.sounds.len(), 1);
        assert_eq!(b.settings.custom_sounds.len(), 1);
        assert_eq!(b.settings.sound, "file:s1");
        assert_eq!(b.settings.wellness.posture.sound, "none");
    }

    #[test]
    fn shortcuts_start_unset() {
        let s = Settings::default();
        assert!(s.shortcuts.take_break.is_empty() && s.shortcuts.toggle_pause.is_empty() && s.shortcuts.skip_break.is_empty());
        assert!(s.shortcuts_opt_in);
    }

    #[test]
    fn old_default_shortcuts_are_cleared_once_custom_ones_stay() {
        let old = r#"{"shortcuts":{"take_break":"CmdOrCtrl+Alt+B","toggle_pause":"CmdOrCtrl+Alt+P","skip_break":"CmdOrCtrl+Shift+K"}}"#;
        let mut s: Settings = serde_json::from_str(old).unwrap();
        assert!(!s.shortcuts_opt_in);
        s.migrate();
        assert_eq!(s.shortcuts.take_break, "");
        assert_eq!(s.shortcuts.toggle_pause, "");
        assert_eq!(s.shortcuts.skip_break, "CmdOrCtrl+Shift+K");
        // after migrating, picking an old default on purpose sticks
        s.shortcuts.toggle_pause = "CmdOrCtrl+Alt+P".into();
        let mut again: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        again.migrate();
        assert_eq!(again.shortcuts.toggle_pause, "CmdOrCtrl+Alt+P");
    }

    #[test]
    fn image_background_needs_an_embedded_image() {
        let b = parse_backup(&backup_json(r#","background":"image","has_image":true"#, "")).unwrap();
        assert!(!b.settings.has_image);
        assert_eq!(b.settings.background, "blur");
        let top = r#","background":"data:image/png;base64,AAAA""#;
        let b = parse_backup(&backup_json(r#","background":"image""#, top)).unwrap();
        assert!(b.settings.has_image);
        assert_eq!(b.settings.background, "image");
    }
}
