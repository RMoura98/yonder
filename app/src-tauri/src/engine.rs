//! The break scheduler: a small state machine ticked once per second.
//! It never touches windows directly; it returns `Effect`s for lib.rs to apply.

use crate::i18n;
use crate::settings::{Reminder, Settings};
use crate::stats::{today_key, Stats};
use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Seconds of inactivity after which screen time stops counting.
const AWAY_SECS: u64 = 60;
/// How long we will politely wait for typing to stop before a break.
const MAX_TYPING_WAIT: u32 = 30;
/// Seconds the Skip button stays disabled in "balanced" difficulty.
pub const BALANCED_SKIP_DELAY: u64 = 5;
/// Fraction of a break after which "End break" replaces Skip.
const END_EARLY_FRACTION: f64 = 0.7;

#[derive(Debug, Clone)]
pub enum Effect {
    ShowPrebreak,
    HidePrebreak,
    StartBreak,
    EndBreak { delay_ms: u64 },
    ShowReminders(Vec<Reminder>),
    Hook { cmd: String, event: &'static str, kind: String, secs: u64 },
    Lock,
    SaveStats,
}

#[derive(Serialize, Clone, Debug)]
pub struct BreakInfo {
    /// short | long | planned | custom | focus
    pub kind: String,
    pub title: String,
    pub icon: String,
    pub message: String,
    pub total_secs: u64,
    pub elapsed_secs: u64,
}

impl BreakInfo {
    pub fn remaining(&self) -> u64 {
        self.total_secs.saturating_sub(self.elapsed_secs)
    }
}

/// Where a running focus session stands (see `Core::start_focus`).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FocusInfo {
    /// 1-based focus period within the session.
    pub cycle: u32,
    pub total: u32,
    /// work | break
    pub phase: &'static str,
    pub secs_left: u64,
    /// The current (or upcoming) break is the session's long one.
    pub long: bool,
}

/// Engine-side state of a focus session. The break itself is an ordinary
/// `BreakInfo` of kind "focus", so only the work phase needs a clock here.
#[derive(Debug, Clone, Default)]
pub struct FocusSession {
    pub cycle: u32,
    /// Active screen seconds spent in the current focus period.
    pub work_elapsed: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    /// working | break | paused | smart | offhours | away
    pub phase: &'static str,
    pub label: String,
    pub work_secs: u64,
    pub interval_secs: u64,
    pub next_break_in: i64,
    pub next_is_long: bool,
    pub prebreak_secs: u64,
    /// epoch seconds, -1 for "until resumed", absent if not paused
    pub paused_until: Option<i64>,
    pub reason: Option<String>,
    #[serde(rename = "break")]
    pub brk: Option<BreakInfo>,
    pub can_skip: bool,
    pub skip_in: u64,
    pub can_end: bool,
    pub skip_difficulty: String,
    /// The break is due but waiting for a pause in typing (up to MAX_TYPING_WAIT seconds).
    pub holding: bool,
    /// Seconds the current due break has been held back by smart pause.
    pub held_secs: u64,
    /// The smart-pause ceiling was hit, so this break is going ahead regardless.
    pub overdue: bool,
    /// A running focus session, if any.
    pub focus: Option<FocusInfo>,
}

#[derive(Default)]
pub struct Engine {
    pub work_secs: u64,
    pub short_count: u32,
    pub snooze_secs: u64,
    pub prebreak_shown: bool,
    pub pause: Option<Option<i64>>,
    pub brk: Option<BreakInfo>,
    pub smart_reason: Option<String>,
    pub smart_hold_until: i64,
    pub away: bool,
    pub offhours: bool,
    pub focus: Option<FocusSession>,
    typing_wait: u32,
    held_secs: u64,
    hold_overdue: bool,
    natural_counted: bool,
    planned_fired: HashSet<String>,
    reminder_elapsed: HashMap<String, u64>,
    pending_reminders: Vec<Reminder>,
    msg_index: usize,
    stats_dirty_secs: u32,
}

/// A snapshot older than this is ignored on startup (the gap counts as a break).
pub const RESTORE_MAX_AGE_SECS: i64 = 10 * 60;

/// The small part of the engine worth surviving a restart, kept in `state.json`.
/// Never describes an active break: those are saved as a fresh countdown.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SavedState {
    pub saved_at: i64,
    pub work_secs: u64,
    pub snooze_secs: u64,
    pub short_count: u32,
    pub paused: bool,
    /// Epoch seconds the pause ends; `None` with `paused` means "until resumed".
    pub pause_until: Option<i64>,
    pub reminder_elapsed: HashMap<String, u64>,
    pub held_secs: u64,
    pub hold_overdue: bool,
}

impl SavedState {
    pub fn load(path: &std::path::Path) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    pub fn save(&self, path: &std::path::Path) {
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
}

pub struct Core {
    pub settings: Settings,
    pub stats: Stats,
    pub e: Engine,
}

fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    Some(h.trim().parse::<u32>().ok()? * 60 + m.trim().parse::<u32>().ok()?)
}

impl Core {
    pub fn new(settings: Settings, stats: Stats) -> Self {
        Self { settings, stats, e: Engine::default() }
    }

    /// Snapshot of the countdown for `state.json`.
    pub fn saved_state(&self) -> SavedState {
        // A break in progress is saved as already done: a restart mid-break
        // shouldn't resume the break or leave a full countdown behind.
        let resting = self.e.brk.is_some();
        SavedState {
            saved_at: now_epoch(),
            work_secs: if resting { 0 } else { self.e.work_secs },
            snooze_secs: if resting { 0 } else { self.e.snooze_secs },
            short_count: self.e.short_count,
            paused: self.e.pause.is_some(),
            pause_until: self.e.pause.flatten(),
            reminder_elapsed: self.e.reminder_elapsed.clone(),
            held_secs: self.e.held_secs,
            hold_overdue: self.e.hold_overdue,
        }
    }

    /// Restore a snapshot at startup. Returns false when it was too old (or from
    /// the future) and the engine starts fresh. The time since the snapshot counts
    /// as away: it is not added to the countdown, and if it is at least the
    /// idle-reset threshold it counts as a natural break.
    pub fn restore(&mut self, saved: &SavedState, now: i64) -> bool {
        let gap = now - saved.saved_at;
        if gap < 0 || gap >= RESTORE_MAX_AGE_SECS {
            return false;
        }
        self.e.short_count = saved.short_count;
        self.e.reminder_elapsed = saved.reminder_elapsed.clone();
        let reset_after = self.settings.idle_reset_mins.max(1) as i64 * 60;
        if gap >= reset_after {
            if saved.work_secs > 0 {
                self.stats.today().natural_breaks += 1;
            }
            self.e.natural_counted = true;
        } else {
            self.e.work_secs = saved.work_secs;
            self.e.snooze_secs = saved.snooze_secs;
            self.e.held_secs = saved.held_secs;
            self.e.hold_overdue = saved.hold_overdue;
        }
        if saved.paused {
            match saved.pause_until {
                None => self.e.pause = Some(None),
                Some(t) if t > now => self.e.pause = Some(Some(t)),
                Some(_) => {}
            }
        }
        true
    }

    pub fn interval_secs(&self) -> u64 {
        self.settings.work_mins.max(1) as u64 * 60 + self.e.snooze_secs
    }

    fn next_is_long(&self) -> bool {
        self.settings.long_enabled && self.e.short_count >= self.settings.long_every.max(1)
    }

    fn focus_total(&self) -> u32 {
        self.settings.focus.long_every.max(1)
    }

    fn focus_work_secs(&self) -> u64 {
        self.settings.focus.work_mins.max(1) as u64 * 60
    }

    /// Seconds until the next break and whether it is a focus break.
    ///
    /// During a focus session an eye break that would land within the merge window of
    /// the focus break (or is due while one is that close) is folded into it, so the
    /// focus break is reported instead and the eye break is never started on its own.
    /// (The merge-window idea comes from EyePomo, which has no license: idea only.)
    fn next_break(&self) -> (i64, bool) {
        let eye = self.interval_secs() as i64 - self.e.work_secs as i64;
        let focus = match (&self.e.focus, &self.e.brk) {
            (Some(f), None) => Some(self.focus_work_secs() as i64 - f.work_elapsed as i64),
            _ => None,
        };
        match focus {
            Some(f) if f <= eye.max(0) + self.settings.focus.merge_window_mins as i64 * 60 => (f, true),
            _ => (eye, false),
        }
    }

    fn in_office_hours(&self) -> bool {
        let oh = &self.settings.office_hours;
        if !oh.enabled {
            return true;
        }
        let now = Local::now();
        let wd = now.weekday().num_days_from_monday() as usize;
        if !oh.days.get(wd).copied().unwrap_or(false) {
            return false;
        }
        let mins = now.hour() * 60 + now.minute();
        match (parse_hm(&oh.start), parse_hm(&oh.end)) {
            (Some(s), Some(e)) if s < e => mins >= s && mins < e,
            (Some(s), Some(e)) if s > e => mins >= s || mins < e, // overnight
            _ => true,
        }
    }

    // ------------------------------------------------------------ snapshot

    pub fn snapshot(&self) -> Snapshot {
        let interval = self.interval_secs();
        let (can_skip, skip_in, can_end) = self.skip_rules();
        let phase = if self.e.brk.is_some() {
            "break"
        } else if self.e.pause.is_some() {
            "paused"
        } else if self.e.offhours {
            "offhours"
        } else if self.e.smart_reason.is_some() {
            "smart"
        } else if self.e.away {
            "away"
        } else {
            "working"
        };
        let (next_in, next_focus) = self.next_break();
        let next_long = if next_focus {
            self.e.focus.as_ref().map_or(false, |f| f.cycle >= self.focus_total())
        } else {
            self.next_is_long()
        };
        let focus = self.e.focus.as_ref().map(|f| {
            let on_break = self.e.brk.as_ref().filter(|b| b.kind == "focus");
            FocusInfo {
                cycle: f.cycle.min(self.focus_total()),
                total: self.focus_total(),
                phase: if on_break.is_some() { "break" } else { "work" },
                secs_left: match on_break {
                    Some(b) => b.remaining(),
                    None => self.focus_work_secs().saturating_sub(f.work_elapsed),
                },
                long: f.cycle >= self.focus_total(),
            }
        });
        let lang = i18n::resolve(&self.settings.language);
        let tr = |key: &str, vars: &[(&str, &str)]| i18n::tr(lang, key, vars);
        let label = match phase {
            "break" => {
                let r = self.e.brk.as_ref().map(|b| b.remaining()).unwrap_or(0);
                tr("On a break — {t}", &[("t", &fmt_dur(r))])
            }
            "paused" => match self.e.pause {
                Some(Some(t)) => {
                    let left = (t - now_epoch()).max(0) as u64;
                    tr("Paused — resumes in {t}", &[("t", &fmt_dur(left))])
                }
                _ => tr("Paused", &[]),
            },
            "offhours" => tr("Outside office hours", &[]),
            // the reason is already translated where it's detected (lib.rs)
            "smart" => tr(
                "Holding breaks — {reason}",
                &[("reason", &self.e.smart_reason.clone().unwrap_or_default())],
            ),
            "away" => tr("You're away — timer on hold", &[]),
            _ => {
                if next_in <= 0 {
                    tr("Break starting soon", &[])
                } else if next_long {
                    tr("Next long break in {t}", &[("t", &fmt_dur(next_in as u64))])
                } else {
                    tr("Next break in {t}", &[("t", &fmt_dur(next_in as u64))])
                }
            }
        };
        Snapshot {
            phase,
            label,
            // The ring on the Today page follows whichever break is next.
            work_secs: match (&self.e.focus, next_focus) {
                (Some(f), true) => f.work_elapsed,
                _ => self.e.work_secs,
            },
            interval_secs: if next_focus { self.focus_work_secs() } else { interval },
            next_break_in: next_in,
            next_is_long: next_long,
            prebreak_secs: self.settings.prebreak_secs as u64,
            paused_until: self.e.pause.map(|p| p.unwrap_or(-1)),
            reason: self.e.smart_reason.clone(),
            brk: self.e.brk.clone(),
            can_skip,
            skip_in,
            can_end,
            skip_difficulty: self.settings.skip_difficulty.clone(),
            held_secs: self.e.held_secs,
            overdue: self.e.hold_overdue,
            focus,
            holding: phase == "working" && next_in <= 0 && self.e.typing_wait > 0,
        }
    }

    /// (can_skip, seconds until skip unlocks, can_end_early)
    fn skip_rules(&self) -> (bool, u64, bool) {
        let Some(b) = &self.e.brk else {
            return (self.settings.skip_difficulty != "hardcore", 0, false);
        };
        let deliberate = b.kind == "planned" || b.kind == "custom";
        let near_done = self.settings.allow_end_early
            && b.elapsed_secs as f64 >= b.total_secs as f64 * END_EARLY_FRACTION;
        if deliberate {
            return (false, 0, true);
        }
        if near_done {
            return (false, 0, true);
        }
        match self.settings.skip_difficulty.as_str() {
            "casual" => (true, 0, false),
            "hardcore" => (false, 0, false),
            // Firm: skippable at once, but the break screen makes you hold the button (see skip_inner).
            "firm" => (true, 0, false),
            _ => {
                let wait = BALANCED_SKIP_DELAY.saturating_sub(b.elapsed_secs);
                (wait == 0, wait, false)
            }
        }
    }

    // ---------------------------------------------------------------- tick

    pub fn tick(&mut self, idle: u64, smart: Option<String>) -> Vec<Effect> {
        let mut fx = Vec::new();
        let now = now_epoch();

        // Smart pause with a cool-down after the activity ends.
        match smart {
            Some(r) => {
                self.e.smart_reason = Some(r);
                self.e.smart_hold_until = now + self.settings.smart.cooldown_secs as i64;
            }
            None if now < self.e.smart_hold_until && self.e.smart_reason.is_some() => {}
            None => {
                self.e.smart_reason = None;
                if !self.e.hold_overdue {
                    self.e.held_secs = 0;
                }
            }
        }

        // Breaks always run down, whatever else happens.
        if let Some(b) = &mut self.e.brk {
            b.elapsed_secs += 1;
            if b.remaining() == 0 {
                self.finish_break(true, &mut fx);
            }
            self.tick_reminders(false, &mut fx);
            return fx;
        }

        // Manual pause.
        if let Some(until) = self.e.pause {
            match until {
                Some(t) if now >= t => self.e.pause = None,
                _ => {
                    self.hide_prebreak(&mut fx);
                    return fx;
                }
            }
        }

        self.e.offhours = !self.in_office_hours();
        if self.e.offhours {
            self.hide_prebreak(&mut fx);
            return fx;
        }

        // Planned breaks (lunch, a walk...) fire at their time of day.
        if self.e.smart_reason.is_none() {
            if let Some(p) = self.due_planned() {
                self.start_break_with(
                    "planned",
                    p.title.clone(),
                    p.icon.clone(),
                    p.duration_mins.max(1) as u64 * 60,
                    &mut fx,
                );
                return fx;
            }
        }

        // Idle handling: away for long enough counts as a natural break.
        let reset_after = self.settings.idle_reset_mins.max(1) as u64 * 60;
        if idle >= reset_after {
            if !self.e.natural_counted && self.e.work_secs > 0 {
                self.e.natural_counted = true;
                self.stats.today().natural_breaks += 1;
                fx.push(Effect::SaveStats);
            }
            self.e.work_secs = 0;
            self.e.snooze_secs = 0;
            self.reset_hold();
            self.e.away = true;
            self.hide_prebreak(&mut fx);
            return fx;
        }
        self.e.away = idle >= AWAY_SECS;
        if self.e.away {
            return fx;
        }
        self.e.natural_counted = false;

        // Active screen time.
        self.e.work_secs += 1;
        {
            let work = self.e.work_secs;
            let day = self.stats.today();
            day.screen_secs += 1;
            day.longest_stretch_secs = day.longest_stretch_secs.max(work);
        }
        self.e.stats_dirty_secs += 1;
        if self.e.stats_dirty_secs >= 60 {
            self.e.stats_dirty_secs = 0;
            fx.push(Effect::SaveStats);
        }
        if let Some(f) = &mut self.e.focus {
            f.work_elapsed += 1;
        }
        self.tick_reminders(true, &mut fx);

        // The next break is either the eye break or (during a focus session) a focus break.
        let (rem, focus_next) = self.next_break();
        let span = if focus_next { self.focus_work_secs() } else { self.interval_secs() };
        if self.e.smart_reason.is_some() && !self.e.hold_overdue {
            // Only a break that is actually due counts as "held".
            let max = self.settings.smart.max_hold_mins as u64 * 60;
            if rem <= 0 {
                self.e.held_secs += 1;
            }
            if max > 0 && self.e.held_secs >= max {
                // Stuck reason: stop honouring it for this break. Rewind so the normal
                // heads-up runs its full lead countdown, then the break starts.
                self.e.hold_overdue = true;
                let lead = self.settings.prebreak_secs as u64;
                if self.settings.prebreak_enabled && lead > 0 && span > lead {
                    match &mut self.e.focus {
                        Some(f) if focus_next => f.work_elapsed = span - lead,
                        _ => self.e.work_secs = span - lead,
                    }
                }
            } else {
                self.hide_prebreak(&mut fx);
                return fx;
            }
        }

        // (the hold above may have rewound the countdown)
        let rem = self.next_break().0;
        let lead = self.settings.prebreak_secs as u64;
        if self.settings.prebreak_enabled
            && !self.e.prebreak_shown
            && lead > 0
            && span > lead
            && rem <= lead as i64
        {
            self.e.prebreak_shown = true;
            fx.push(Effect::ShowPrebreak);
        }

        if rem <= 0 {
            if self.settings.wait_while_typing && idle < 2 && self.e.typing_wait < MAX_TYPING_WAIT {
                self.e.typing_wait += 1;
            } else {
                self.start_scheduled_break(&mut fx);
            }
        }
        fx
    }

    fn due_planned(&mut self) -> Option<crate::settings::PlannedBreak> {
        let now = Local::now();
        let wd = now.weekday().num_days_from_monday() as usize;
        let mins = now.hour() * 60 + now.minute();
        let today = today_key();
        for p in &self.settings.planned {
            if !p.enabled || !p.days.get(wd).copied().unwrap_or(false) {
                continue;
            }
            let Some(at) = parse_hm(&p.time) else { continue };
            // fire within a 30-minute window so a meeting can delay it
            if mins >= at && mins < at + 30 {
                let key = format!("{}@{}", p.id, today);
                if self.e.planned_fired.insert(key) {
                    return Some(p.clone());
                }
            }
        }
        None
    }

    fn tick_reminders(&mut self, active: bool, fx: &mut Vec<Effect>) {
        let on_break = self.e.brk.is_some();
        if active || on_break {
            let due: Vec<Reminder> = self
                .settings
                .all_reminders()
                .into_iter()
                .filter(|r| r.enabled && r.interval_mins > 0)
                .filter_map(|r| {
                    let el = self.e.reminder_elapsed.entry(r.id.clone()).or_insert(0);
                    *el += 1;
                    if *el >= r.interval_mins as u64 * 60 {
                        *el = 0;
                        Some(r.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for r in due {
                if !self.e.pending_reminders.iter().any(|p| p.id == r.id) {
                    self.e.pending_reminders.push(r);
                }
            }
        }
        // Smart pause (a call, a fullscreen game, Do Not Disturb…) holds wellness
        // reminders too; they stay pending (at most one per kind) and show once it ends.
        let held = self.e.smart_reason.is_some() && !self.e.hold_overdue;
        if !on_break && !held && !self.e.pending_reminders.is_empty() {
            let list = std::mem::take(&mut self.e.pending_reminders);
            self.stats.today().reminders_shown += list.len() as u32;
            if self.settings.wellness.combine {
                fx.push(Effect::ShowReminders(list));
            } else {
                for r in list {
                    fx.push(Effect::ShowReminders(vec![r]));
                }
            }
        }
    }

    fn reset_hold(&mut self) {
        self.e.held_secs = 0;
        self.e.hold_overdue = false;
    }

    fn hide_prebreak(&mut self, fx: &mut Vec<Effect>) {
        if self.e.prebreak_shown {
            self.e.prebreak_shown = false;
            fx.push(Effect::HidePrebreak);
        }
    }

    fn next_message(&mut self) -> String {
        let msgs: Vec<&String> = self
            .settings
            .messages
            .iter()
            .filter(|m| !m.trim().is_empty())
            .collect();
        if msgs.is_empty() {
            return "Rest your eyes for a moment.".into();
        }
        self.e.msg_index = (self.e.msg_index + 1) % msgs.len();
        msgs[self.e.msg_index].clone()
    }

    fn start_focus_break(&mut self, fx: &mut Vec<Effect>) {
        let long = self.e.focus.as_ref().map_or(false, |f| f.cycle >= self.focus_total());
        let f = &self.settings.focus;
        let (title, mins) = if long {
            ("Long focus break", f.long_break_mins)
        } else {
            ("Focus break", f.break_mins)
        };
        self.start_break_with("focus", title.into(), "timer".into(), mins.max(1) as u64 * 60, fx);
    }

    /// Moves the session on after a focus break (or a skipped one); ends it after the last.
    fn advance_focus(&mut self) {
        let total = self.focus_total();
        if let Some(f) = &mut self.e.focus {
            if f.cycle >= total {
                self.e.focus = None;
            } else {
                f.cycle += 1;
                f.work_elapsed = 0;
            }
        }
    }

    fn start_scheduled_break(&mut self, fx: &mut Vec<Effect>) {
        if self.next_break().1 {
            return self.start_focus_break(fx);
        }
        let long = self.next_is_long();
        let (kind, title, secs) = if long {
            ("long", "Long break".to_string(), self.settings.long_mins.max(1) as u64 * 60)
        } else {
            ("short", "Time for a break".to_string(), self.settings.break_secs.max(5) as u64)
        };
        self.start_break_with(kind, title, String::new(), secs, fx);
    }

    fn start_break_with(&mut self, kind: &str, title: String, icon: String, secs: u64, fx: &mut Vec<Effect>) {
        if self.e.brk.is_some() {
            return;
        }
        if self.e.prebreak_shown {
            self.e.prebreak_shown = false;
            fx.push(Effect::HidePrebreak);
        }
        let message = self.next_message();
        self.e.brk = Some(BreakInfo {
            kind: kind.into(),
            title,
            icon,
            message,
            total_secs: secs,
            elapsed_secs: 0,
        });
        self.e.typing_wait = 0;
        self.reset_hold();
        fx.push(Effect::StartBreak);
        if self.settings.lock_on_break {
            fx.push(Effect::Lock);
        }
        fx.push(Effect::Hook {
            cmd: self.settings.hooks.on_break_start.clone(),
            event: "break-start",
            kind: kind.into(),
            secs,
        });
    }

    fn finish_break(&mut self, taken: bool, fx: &mut Vec<Effect>) {
        let Some(b) = self.e.brk.take() else { return };
        match b.kind.as_str() {
            "long" => self.e.short_count = 0,
            "short" => self.e.short_count += 1,
            "focus" => self.advance_focus(),
            _ => {}
        }
        // A deliberate break doesn't reset the countdown when skipped.
        let resume_countdown = !taken && b.kind == "custom";
        if !resume_countdown {
            self.e.work_secs = 0;
            self.e.snooze_secs = 0;
        }
        self.e.prebreak_shown = false;
        self.e.typing_wait = 0;
        self.reset_hold();
        {
            let day = self.stats.today();
            if taken {
                day.breaks_taken += 1;
            } else {
                day.breaks_skipped += 1;
            }
        }
        if self.settings.wellness.reset_after_break && taken {
            self.e.reminder_elapsed.clear();
        }
        fx.push(Effect::EndBreak { delay_ms: if taken { 1500 } else { 950 } });
        fx.push(Effect::SaveStats);
        fx.push(Effect::Hook {
            cmd: self.settings.hooks.on_break_end.clone(),
            event: if taken { "break-end" } else { "break-skip" },
            kind: b.kind,
            secs: b.elapsed_secs,
        });
    }

    // ------------------------------------------------------------- actions

    pub fn take_break_now(&mut self, mins: Option<u32>) -> Vec<Effect> {
        let mut fx = Vec::new();
        let secs = match mins {
            Some(m) if m > 0 => m as u64 * 60,
            _ => self.settings.break_secs.max(5) as u64,
        };
        self.start_break_with("custom", "Break time".into(), String::new(), secs, &mut fx);
        fx
    }

    /// Skip or end the current break, honoring the difficulty rules.
    pub fn skip_or_end_break(&mut self) -> Vec<Effect> {
        self.skip_inner(false)
    }

    /// The Skip button of the break screen was held long enough (Firm mode).
    pub fn skip_held(&mut self) -> Vec<Effect> {
        self.skip_inner(true)
    }

    fn skip_inner(&mut self, held: bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        let (mut can_skip, _, can_end) = self.skip_rules();
        // Firm: a plain skip (Esc, tray, shortcut) is refused while the break screen is up,
        // since that one has the hold-to-skip button. Styles without clickable buttons
        // (gentle, notification) have nothing to hold, so the tray / shortcut still skip.
        if self.settings.skip_difficulty == "firm" && !held && !matches!(self.settings.break_style.as_str(), "gentle" | "notify") {
            can_skip = false;
        }
        if can_end {
            let deliberate = matches!(self.e.brk.as_ref().map(|b| b.kind.as_str()), Some("planned" | "custom"));
            // A deliberate break counts as taken (and resets the countdown) once it lasted
            // at least a normal break, or most of its own length.
            let normal = self.settings.break_secs.max(5) as u64;
            let taken = !deliberate
                || self.e.brk.as_ref().map(|b| {
                    b.elapsed_secs >= normal || b.elapsed_secs as f64 >= b.total_secs as f64 * END_EARLY_FRACTION
                }).unwrap_or(false);
            self.finish_break(taken, &mut fx);
        } else if can_skip {
            self.finish_break(false, &mut fx);
        }
        fx
    }

    /// Skip the upcoming break (from the heads-up or the tray).
    pub fn skip_next(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.e.brk.is_some() {
            return self.skip_or_end_break();
        }
        if self.settings.skip_difficulty == "hardcore" {
            return fx;
        }
        let focus_next = self.next_break().1;
        self.e.work_secs = 0;
        self.e.snooze_secs = 0;
        self.reset_hold();
        if focus_next {
            self.advance_focus();
        } else {
            self.e.short_count += 1;
        }
        self.stats.today().breaks_skipped += 1;
        self.hide_prebreak(&mut fx);
        fx.push(Effect::SaveStats);
        fx
    }

    pub fn snooze(&mut self, mins: u32) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.settings.skip_difficulty == "hardcore" {
            return fx;
        }
        if self.next_break().1 {
            let total = self.focus_work_secs();
            if let Some(f) = &mut self.e.focus {
                f.work_elapsed = total.saturating_sub(mins as u64 * 60);
            }
            self.reset_hold();
            self.hide_prebreak(&mut fx);
            return fx;
        }
        // The next break now happens `mins` minutes from now (counting active time).
        let base = self.settings.work_mins.max(1) as u64 * 60;
        let target = self.e.work_secs + mins as u64 * 60;
        self.e.snooze_secs = target.saturating_sub(base);
        self.reset_hold();
        self.hide_prebreak(&mut fx);
        // Don't nag again right away for short snoozes.
        if (mins as u64 * 60) <= self.settings.prebreak_secs as u64 + 5 {
            self.e.prebreak_shown = true;
        }
        fx
    }

    pub fn start_now(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.start_scheduled_break(&mut fx);
        fx
    }

    pub fn pause(&mut self, mins: Option<i64>) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.e.pause = Some(mins.map(|m| now_epoch() + m * 60));
        self.hide_prebreak(&mut fx);
        fx
    }

    pub fn pause_until_tomorrow(&mut self) -> Vec<Effect> {
        let tomorrow = Local::now().date_naive().succ_opt().and_then(|d| d.and_hms_opt(0, 0, 0));
        let mut fx = Vec::new();
        if let Some(t) = tomorrow.and_then(|t| t.and_local_timezone(Local).single()) {
            self.e.pause = Some(Some(t.timestamp()));
        } else {
            self.e.pause = Some(None);
        }
        self.hide_prebreak(&mut fx);
        fx
    }

    pub fn resume(&mut self) {
        self.e.pause = None;
    }

    /// Starts a focus session (cycle 1). The eye-break countdown keeps running and is
    /// merged with the focus breaks, see `next_break`.
    pub fn start_focus(&mut self) {
        self.e.focus = Some(FocusSession { cycle: 1, work_elapsed: 0 });
    }

    /// Stops the session; the normal eye-break schedule carries on untouched.
    pub fn stop_focus(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.e.focus = None;
        // A heads-up for a focus break that no longer exists.
        self.hide_prebreak(&mut fx);
        fx
    }

    pub fn toggle_focus(&mut self) -> Vec<Effect> {
        if self.e.focus.is_some() {
            self.stop_focus()
        } else {
            self.start_focus();
            Vec::new()
        }
    }

    pub fn reminder_done(&mut self) {
        self.stats.today().reminders_done += 1;
    }
}

pub fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn fmt_dur(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_focus_tests.rs"]
mod focus_tests;
