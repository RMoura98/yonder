//! Unit tests for focus sessions (included from engine.rs).

use super::*;

fn test_settings() -> Settings {
    let mut s = Settings::default();
    s.work_mins = 1;
    s.break_secs = 20;
    s.prebreak_enabled = false;
    s.prebreak_secs = 10;
    s.wait_while_typing = false;
    s.skip_difficulty = "balanced".into();
    s.allow_end_early = true;
    s.long_enabled = false;
    s.idle_reset_mins = 2;
    s.office_hours.enabled = false;
    s.planned.clear();
    s.wellness.posture.enabled = false;
    s.wellness.blink.enabled = false;
    s.wellness.custom.clear();
    s
}

fn tick_n(c: &mut Core, n: u32, idle: u64) -> Vec<Effect> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(c.tick(idle, None));
    }
    all
}

fn starts(fx: &[Effect]) -> usize {
    fx.iter().filter(|e| matches!(e, Effect::StartBreak)).count()
}
fn ends(fx: &[Effect]) -> usize {
    fx.iter().filter(|e| matches!(e, Effect::EndBreak { .. })).count()
}
fn shows(fx: &[Effect]) -> usize {
    fx.iter().filter(|e| matches!(e, Effect::ShowPrebreak)).count()
}

fn work_until_break(c: &mut Core) -> String {
    for _ in 0..100_000 {
        if starts(&c.tick(0, None)) > 0 {
            return c.e.brk.as_ref().unwrap().kind.clone();
        }
    }
    panic!("no break started");
}

/// A core with a running focus session (25/5, 15 long, 4 cycles, 3 min merge window)
/// whose eye breaks come every `eye_mins` minutes.
fn focus_core(eye_mins: u32) -> Core {
    let mut s = test_settings();
    s.work_mins = eye_mins;
    let mut c = Core::new(s, Stats::default());
    c.start_focus();
    c
}

fn break_kind(c: &Core) -> Option<String> {
    c.e.brk.as_ref().map(|b| b.kind.clone())
}

#[test]
fn focus_settings_default_and_load_from_empty() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.focus.work_mins, 25);
    assert_eq!(s.focus.break_mins, 5);
    assert_eq!(s.focus.long_break_mins, 15);
    assert_eq!(s.focus.long_every, 4);
    assert_eq!(s.focus.merge_window_mins, 3);
}

#[test]
fn no_focus_info_without_a_session() {
    let c = Core::new(test_settings(), Stats::default());
    assert!(c.snapshot().focus.is_none());
}

#[test]
fn focus_break_starts_after_work_mins() {
    let mut c = focus_core(200);
    let f = c.snapshot().focus.unwrap();
    assert_eq!((f.cycle, f.total, f.phase, f.secs_left), (1, 4, "work", 1500));
    assert_eq!(starts(&tick_n(&mut c, 1499, 0)), 0);
    assert_eq!(c.snapshot().focus.unwrap().secs_left, 1);
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
    let b = c.e.brk.as_ref().unwrap();
    assert_eq!((b.kind.as_str(), b.title.as_str(), b.total_secs), ("focus", "Focus break", 300));
    let f = c.snapshot().focus.unwrap();
    assert_eq!((f.cycle, f.phase, f.secs_left, f.long), (1, "break", 300, false));
    // after the break the next focus period begins
    assert_eq!(ends(&tick_n(&mut c, 300, 0)), 1);
    assert_eq!(c.stats.today().breaks_taken, 1);
    let f = c.snapshot().focus.unwrap();
    assert_eq!((f.cycle, f.phase, f.secs_left), (2, "work", 1500));
    // a focus break doesn't count as a short eye break
    assert_eq!(c.e.short_count, 0);
}

#[test]
fn long_focus_break_every_fourth_and_session_ends() {
    let mut c = focus_core(200);
    for cycle in 1..=3 {
        assert_eq!(work_until_break(&mut c), "focus");
        assert_eq!(c.e.brk.as_ref().unwrap().total_secs, 300, "cycle {cycle}");
        tick_n(&mut c, 300, 0);
        assert_eq!(c.snapshot().focus.unwrap().cycle, cycle + 1);
    }
    assert_eq!(work_until_break(&mut c), "focus");
    let b = c.e.brk.as_ref().unwrap();
    assert_eq!((b.title.as_str(), b.total_secs), ("Long focus break", 900));
    let f = c.snapshot().focus.unwrap();
    assert_eq!((f.cycle, f.long), (4, true));
    tick_n(&mut c, 900, 0);
    // last cycle done: session over, normal schedule continues
    assert!(c.snapshot().focus.is_none());
    assert_eq!(c.stats.today().breaks_taken, 4);
}

#[test]
fn eye_break_near_focus_break_is_folded_into_it() {
    // eye break due at 23 min, focus break at 25 min: 2 min apart, inside the 3 min window
    let mut c = focus_core(23);
    c.settings.prebreak_enabled = true;
    let fx = tick_n(&mut c, 1499, 0);
    assert_eq!(starts(&fx), 0, "eye break must not fire on its own");
    assert_eq!(shows(&fx), 1, "a single heads-up, for the focus break");
    assert!(c.e.brk.is_none());
    let fx = tick_n(&mut c, 1, 0);
    assert_eq!(starts(&fx), 1);
    assert_eq!(break_kind(&c).as_deref(), Some("focus"));
    // the focus break restarted the eye countdown too
    tick_n(&mut c, 300, 0);
    assert_eq!(c.e.work_secs, 0);
    assert_eq!(c.stats.today().breaks_taken, 1);
}

#[test]
fn eye_break_far_from_focus_break_is_not_folded() {
    // eye break due at 20 min, focus break at 25 min: 5 min apart, outside the window
    let mut c = focus_core(20);
    assert_eq!(starts(&tick_n(&mut c, 1199, 0)), 0);
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
    assert_eq!(break_kind(&c).as_deref(), Some("short"));
    // the eye break doesn't consume focus time and isn't a focus break
    tick_n(&mut c, 20, 0);
    let f = c.snapshot().focus.unwrap();
    assert_eq!((f.cycle, f.phase, f.secs_left), (1, "work", 300));
    assert_eq!(starts(&tick_n(&mut c, 299, 0)), 0);
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
    assert_eq!(break_kind(&c).as_deref(), Some("focus"));
}

#[test]
fn merge_window_is_configurable() {
    // 5 min apart is outside the default window but inside a 6 min one
    let mut c = focus_core(20);
    c.settings.focus.merge_window_mins = 6;
    assert_eq!(starts(&tick_n(&mut c, 1499, 0)), 0);
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
    assert_eq!(break_kind(&c).as_deref(), Some("focus"));
}

#[test]
fn stopping_restores_the_normal_schedule() {
    let mut c = focus_core(23);
    // eye break (due at 23 min) is being held back for the focus break at 25 min
    assert_eq!(starts(&tick_n(&mut c, 1400, 0)), 0);
    assert_eq!(c.snapshot().focus.unwrap().secs_left, 100);
    let _ = c.stop_focus();
    assert!(c.snapshot().focus.is_none());
    // the overdue eye break fires straight away, as a normal one
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
    assert_eq!(break_kind(&c).as_deref(), Some("short"));
    tick_n(&mut c, 20, 0);
    // and no focus break ever follows
    assert_eq!(work_until_break(&mut c), "short");
}

#[test]
fn toggle_focus_starts_and_stops() {
    let mut c = Core::new(test_settings(), Stats::default());
    c.toggle_focus();
    assert_eq!(c.snapshot().focus.unwrap().cycle, 1);
    c.toggle_focus();
    assert!(c.snapshot().focus.is_none());
}

#[test]
fn pause_freezes_the_focus_session() {
    let mut c = focus_core(200);
    tick_n(&mut c, 100, 0);
    assert_eq!(c.snapshot().focus.unwrap().secs_left, 1400);
    c.pause(Some(30));
    assert_eq!(starts(&tick_n(&mut c, 5000, 0)), 0);
    assert_eq!(c.snapshot().focus.unwrap().secs_left, 1400);
    c.resume();
    tick_n(&mut c, 10, 0);
    assert_eq!(c.snapshot().focus.unwrap().secs_left, 1390);
}

#[test]
fn being_away_freezes_the_focus_session() {
    let mut c = focus_core(200);
    tick_n(&mut c, 100, 0);
    // away (but not long enough to reset): time doesn't count
    tick_n(&mut c, 50, 60);
    assert_eq!(c.snapshot().focus.unwrap().secs_left, 1400);
}

#[test]
fn smart_pause_holds_a_due_focus_break() {
    let mut c = focus_core(200);
    tick_n(&mut c, 1499, 0);
    let mut fx = Vec::new();
    for _ in 0..30 {
        fx.extend(c.tick(0, Some("you're in a call".into())));
    }
    assert_eq!(starts(&fx), 0);
    assert!(c.e.brk.is_none());
    assert_eq!(c.snapshot().phase, "smart");
}

#[test]
fn focus_break_can_be_skipped_and_still_advances() {
    let mut s = test_settings();
    s.work_mins = 200;
    s.skip_difficulty = "casual".into();
    let mut c = Core::new(s, Stats::default());
    c.start_focus();
    assert_eq!(work_until_break(&mut c), "focus");
    c.skip_or_end_break();
    assert!(c.e.brk.is_none());
    assert_eq!(c.stats.today().breaks_skipped, 1);
    assert_eq!(c.snapshot().focus.unwrap().cycle, 2);
}

#[test]
fn skipping_the_upcoming_focus_break_advances_the_session() {
    let mut c = focus_core(200);
    tick_n(&mut c, 1450, 0);
    c.skip_next();
    let f = c.snapshot().focus.unwrap();
    assert_eq!((f.cycle, f.secs_left), (2, 1500));
    assert_eq!(c.e.short_count, 0);
}

#[test]
fn stopping_mid_break_lets_the_break_finish() {
    let mut c = focus_core(200);
    assert_eq!(work_until_break(&mut c), "focus");
    let _ = c.stop_focus();
    assert!(c.snapshot().focus.is_none());
    assert_eq!(ends(&tick_n(&mut c, 300, 0)), 1);
    assert!(c.snapshot().focus.is_none());
}

#[test]
fn focus_can_override_the_work_period_for_one_session() {
    let mut c = Core::new(test_settings(), Stats::default());
    c.start_focus_for(Some(7));
    assert_eq!(c.focus_work_secs(), 7 * 60);
    c.stop_focus();
    c.start_focus_for(None);
    assert_eq!(c.focus_work_secs(), c.settings.focus.work_mins as u64 * 60);
}
