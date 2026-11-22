use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DayStats {
    pub screen_secs: u64,
    pub breaks_taken: u32,
    pub breaks_skipped: u32,
    pub natural_breaks: u32,
    pub reminders_shown: u32,
    pub reminders_done: u32,
    pub longest_stretch_secs: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Stats {
    pub days: BTreeMap<String, DayStats>,
}

pub fn today_key() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

impl Stats {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string(self) {
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub fn today(&mut self) -> &mut DayStats {
        let key = today_key();
        // keep about a year of history
        if self.days.len() > 400 {
            if let Some(first) = self.days.keys().next().cloned() {
                self.days.remove(&first);
            }
        }
        self.days.entry(key).or_default()
    }
}
