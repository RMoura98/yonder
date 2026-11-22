use serde::{Deserialize, Serialize};
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
    pub apps: Vec<String>,
    pub cooldown_secs: u32,
}

impl Default for SmartPause {
    fn default() -> Self {
        Self {
            enabled: true,
            meetings: true,
            fullscreen: true,
            apps: vec![],
            cooldown_secs: 60,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Shortcuts {
    pub take_break: String,
    pub toggle_pause: String,
    pub skip_break: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            take_break: "CmdOrCtrl+Alt+B".into(),
            toggle_pause: "CmdOrCtrl+Alt+P".into(),
            skip_break: "CmdOrCtrl+Alt+S".into(),
        }
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

    pub prebreak_enabled: bool,
    pub prebreak_secs: u32,
    pub prebreak_visible_secs: u32,

    pub messages: Vec<String>,
    /// fullscreen | split
    pub break_style: String,
    /// aurora | nebula | ripple | gradient | image
    pub background: String,
    pub gradient: String,
    pub has_image: bool,
    pub texture: String,
    pub sound_enabled: bool,
    pub sound: String,
    pub volume: f32,

    pub idle_reset_mins: u32,
    pub smart: SmartPause,
    pub office_hours: OfficeHours,
    pub planned: Vec<PlannedBreak>,
    pub wellness: Wellness,
    pub shortcuts: Shortcuts,
    pub hooks: Hooks,
    pub launch_at_login: bool,
    /// system | light | dark
    pub theme: String,
    pub onboarded: bool,
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
            prebreak_enabled: true,
            prebreak_secs: 60,
            prebreak_visible_secs: 10,
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
            idle_reset_mins: 5,
            smart: SmartPause::default(),
            office_hours: OfficeHours::default(),
            planned: vec![],
            wellness: Wellness::default(),
            shortcuts: Shortcuts::default(),
            hooks: Hooks::default(),
            launch_at_login: true,
            theme: "system".into(),
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Self {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.migrate();
        s
    }

    /// Older builds stored emoji as icons; icons are now Material Symbol names.
    fn migrate(&mut self) {
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
