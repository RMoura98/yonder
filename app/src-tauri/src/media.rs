//! Pause whatever is playing when a break starts and resume exactly what we
//! paused when it ends. Best effort: every failure is silent.
//!
//! * Windows: WinRT GlobalSystemMediaTransportControls sessions.
//! * Linux: MPRIS through the optional `playerctl` tool.
//! * macOS: AppleScript for Music and Spotify (no public system-wide API).

use std::sync::Mutex;

/// Players / sessions we paused and still owe a resume. The lock is held for
/// the whole pause or resume run, so the two can never interleave.
static PAUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Pause playing media (off the calling thread) and remember what we paused.
pub fn pause_playing() {
    std::thread::spawn(|| {
        let _ = std::panic::catch_unwind(|| {
            let mut paused = PAUSED.lock().unwrap_or_else(|e| e.into_inner());
            for id in imp::pause_all() {
                if !paused.contains(&id) {
                    paused.push(id);
                }
            }
        });
    });
}

/// Resume only what `pause_playing` paused.
pub fn resume_paused() {
    std::thread::spawn(|| {
        let _ = std::panic::catch_unwind(|| {
            let mut paused = PAUSED.lock().unwrap_or_else(|e| e.into_inner());
            let ids = std::mem::take(&mut *paused);
            if !ids.is_empty() {
                imp::resume(&ids);
            }
        });
    });
}

/// `playerctl -l` output -> player names.
#[allow(dead_code)]
fn parse_players(out: &str) -> Vec<String> {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// MPRIS / AppleScript state strings: only "playing" counts.
#[allow(dead_code)]
fn is_playing(status: &str) -> bool {
    status.trim().eq_ignore_ascii_case("playing")
}

/// Same for "paused" (avoids resuming something the user already resumed).
#[allow(dead_code)]
fn is_paused(status: &str) -> bool {
    status.trim().eq_ignore_ascii_case("paused")
}

// ------------------------------------------------------------------ Windows
#[cfg(windows)]
mod imp {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    fn manager() -> Option<Manager> {
        Manager::RequestAsync().ok()?.get().ok()
    }

    pub fn pause_all() -> Vec<String> {
        let mut out = Vec::new();
        let Some(mgr) = manager() else { return out };
        let Ok(sessions) = mgr.GetSessions() else { return out };
        for s in sessions {
            let playing = s
                .GetPlaybackInfo()
                .and_then(|i| i.PlaybackStatus())
                .map(|st| st == Status::Playing)
                .unwrap_or(false);
            if !playing {
                continue;
            }
            let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            if let Ok(op) = s.TryPauseAsync() {
                if op.get().unwrap_or(false) {
                    out.push(id);
                }
            }
        }
        out
    }

    pub fn resume(ids: &[String]) {
        let Some(mgr) = manager() else { return };
        let Ok(sessions) = mgr.GetSessions() else { return };
        for s in sessions {
            let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
            if !ids.contains(&id) {
                continue;
            }
            // Don't touch it if the user already resumed it themselves.
            let paused = s
                .GetPlaybackInfo()
                .and_then(|i| i.PlaybackStatus())
                .map(|st| st == Status::Paused)
                .unwrap_or(false);
            if paused {
                if let Ok(op) = s.TryPlayAsync() {
                    let _ = op.get();
                }
            }
        }
    }
}

// -------------------------------------------------------------------- Linux
#[cfg(target_os = "linux")]
mod imp {
    use super::{is_paused, is_playing, parse_players};
    use std::process::{Command, Stdio};

    fn playerctl(args: &[&str]) -> Option<String> {
        let o = Command::new("playerctl").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    pub fn pause_all() -> Vec<String> {
        let mut out = Vec::new();
        let Some(list) = playerctl(&["-l"]) else { return out };
        for p in parse_players(&list) {
            if playerctl(&["-p", &p, "status"]).is_some_and(|s| is_playing(&s))
                && playerctl(&["-p", &p, "pause"]).is_some()
            {
                out.push(p);
            }
        }
        out
    }

    pub fn resume(ids: &[String]) {
        let Some(list) = playerctl(&["-l"]) else { return };
        let alive = parse_players(&list);
        for p in ids {
            if alive.contains(p) && playerctl(&["-p", p, "status"]).is_some_and(|s| is_paused(&s)) {
                let _ = playerctl(&["-p", p, "play"]);
            }
        }
    }
}

// -------------------------------------------------------------------- macOS
#[cfg(target_os = "macos")]
mod imp {
    use super::{is_paused, is_playing};
    use std::process::Command;

    const APPS: [&str; 2] = ["Music", "Spotify"];

    fn osa(script: &str) -> Option<String> {
        let o = Command::new("osascript").arg("-e").arg(script).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    fn running(app: &str) -> bool {
        osa(&format!("tell application \"System Events\" to (name of processes) contains \"{app}\""))
            .is_some_and(|s| s.trim() == "true")
    }

    fn state(app: &str) -> Option<String> {
        osa(&format!("tell application \"{app}\" to player state as string"))
    }

    pub fn pause_all() -> Vec<String> {
        let mut out = Vec::new();
        for app in APPS {
            if running(app)
                && state(app).is_some_and(|s| is_playing(&s))
                && osa(&format!("tell application \"{app}\" to pause")).is_some()
            {
                out.push(app.to_string());
            }
        }
        out
    }

    pub fn resume(ids: &[String]) {
        for app in APPS {
            if ids.iter().any(|i| i == app) && running(app) && state(app).is_some_and(|s| is_paused(&s)) {
                let _ = osa(&format!("tell application \"{app}\" to play"));
            }
        }
    }
}

// ----------------------------------------------------------------- fallback
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    pub fn pause_all() -> Vec<String> {
        Vec::new()
    }
    pub fn resume(_ids: &[String]) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_player_list() {
        assert_eq!(parse_players("spotify\n\n vlc.instance123 \n"), vec!["spotify", "vlc.instance123"]);
        assert!(parse_players("").is_empty());
    }

    #[test]
    fn only_playing_counts() {
        assert!(is_playing("Playing\n"));
        assert!(is_playing("playing"));
        assert!(!is_playing("Paused\n"));
        assert!(!is_playing("Stopped"));
        assert!(!is_playing(""));
        assert!(is_paused("Paused\n"));
    }

    /// Manual: pauses and resumes whatever is playing right now.
    #[test]
    #[ignore]
    fn live_pause_resume() {
        let ids = imp::pause_all();
        println!("paused: {ids:?}");
        std::thread::sleep(std::time::Duration::from_secs(3));
        imp::resume(&ids);
    }
}
