//! Unit tests for the break scheduler (included from engine.rs).

use super::*;

/// Settings with a 60 s work interval, 20 s breaks and a 10 s heads-up.
/// Everything that reads the local clock (office hours, planned breaks)
/// or adds noise (wellness reminders) is switched off.
fn test_settings() -> Settings {
    let mut s = Settings::default();
    s.work_mins = 1;
    s.break_secs = 20;
    s.prebreak_enabled = true;
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

fn core(s: Settings) -> Core {
    Core::new(s, Stats::default())
}

/// Tick `n` times with the given idle time and no smart pause.
fn tick_n(c: &mut Core, n: u32, idle: u64) -> Vec<Effect> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(c.tick(idle, None));
    }
    all
}

fn count(fx: &[Effect], f: impl Fn(&Effect) -> bool) -> usize {
    fx.iter().filter(|e| f(e)).count()
}
fn starts(fx: &[Effect]) -> usize {
    count(fx, |e| matches!(e, Effect::StartBreak))
}
fn ends(fx: &[Effect]) -> usize {
    count(fx, |e| matches!(e, Effect::EndBreak { .. }))
}
fn shows(fx: &[Effect]) -> usize {
    count(fx, |e| matches!(e, Effect::ShowPrebreak))
}
fn hides(fx: &[Effect]) -> usize {
    count(fx, |e| matches!(e, Effect::HidePrebreak))
}
fn taken(c: &mut Core) -> u32 {
    c.stats.today().breaks_taken
}

/// Work until a break starts, returning its kind.
fn work_until_break(c: &mut Core) -> String {
    for _ in 0..10_000 {
        if starts(&c.tick(0, None)) > 0 {
            return c.e.brk.as_ref().unwrap().kind.clone();
        }
    }
    panic!("no break started");
}

#[test]
fn break_starts_after_work_mins() {
    let mut c = core(test_settings());
    assert_eq!(starts(&tick_n(&mut c, 59, 0)), 0);
    assert!(c.e.brk.is_none());
    let fx = tick_n(&mut c, 1, 0);
    assert_eq!(starts(&fx), 1);
    let b = c.e.brk.as_ref().unwrap();
    assert_eq!(b.kind, "short");
    assert_eq!(b.total_secs, 20);
    assert_eq!(c.snapshot().phase, "break");
}

#[test]
fn prebreak_fires_lead_secs_before_break() {
    let mut c = core(test_settings());
    assert_eq!(shows(&tick_n(&mut c, 49, 0)), 0);
    // work_secs 50 = 60 - prebreak_secs
    assert_eq!(shows(&tick_n(&mut c, 1, 0)), 1);
    // only once
    let fx = tick_n(&mut c, 9, 0);
    assert_eq!(shows(&fx), 0);
    assert_eq!(starts(&fx), 0);
    // starting the break hides the heads-up
    let fx = tick_n(&mut c, 1, 0);
    assert_eq!(starts(&fx), 1);
    assert_eq!(hides(&fx), 1);
}

#[test]
fn prebreak_can_be_disabled() {
    let mut s = test_settings();
    s.prebreak_enabled = false;
    let mut c = core(s);
    assert_eq!(shows(&tick_n(&mut c, 60, 0)), 0);
}

#[test]
fn break_runs_down_and_ends() {
    let mut c = core(test_settings());
    tick_n(&mut c, 60, 0);
    assert!(c.e.brk.is_some());
    let fx = tick_n(&mut c, 19, 0);
    assert_eq!(ends(&fx), 0);
    assert_eq!(c.e.brk.as_ref().unwrap().remaining(), 1);
    assert_eq!(taken(&mut c), 0);
    let fx = tick_n(&mut c, 1, 0);
    assert_eq!(ends(&fx), 1);
    assert!(c.e.brk.is_none());
    assert_eq!(taken(&mut c), 1);
    assert_eq!(c.stats.today().breaks_skipped, 0);
    // countdown restarts
    assert_eq!(c.e.work_secs, 0);
    assert_eq!(c.e.short_count, 1);
    assert_eq!(c.snapshot().phase, "working");
}

#[test]
fn breaks_keep_running_while_idle() {
    let mut c = core(test_settings());
    tick_n(&mut c, 60, 0);
    let fx = tick_n(&mut c, 20, 500);
    assert_eq!(ends(&fx), 1);
    assert_eq!(taken(&mut c), 1);
}

#[test]
fn idle_between_away_and_reset_pauses_counting() {
    let mut c = core(test_settings());
    tick_n(&mut c, 30, 0);
    assert_eq!(c.e.work_secs, 30);
    tick_n(&mut c, 20, 60);
    assert_eq!(c.e.work_secs, 30);
    assert!(c.e.away);
    assert_eq!(c.snapshot().phase, "away");
    assert_eq!(c.stats.today().natural_breaks, 0);
    // coming back resumes where we left off
    tick_n(&mut c, 1, 0);
    assert_eq!(c.e.work_secs, 31);
    assert!(!c.e.away);
}

#[test]
fn idle_past_reset_threshold_counts_a_natural_break() {
    let mut c = core(test_settings());
    tick_n(&mut c, 30, 0);
    let fx = tick_n(&mut c, 1, 120); // idle_reset_mins = 2
    assert_eq!(c.e.work_secs, 0);
    assert_eq!(c.stats.today().natural_breaks, 1);
    assert!(count(&fx, |e| matches!(e, Effect::SaveStats)) >= 1);
    // staying away doesn't count it again
    tick_n(&mut c, 10, 200);
    assert_eq!(c.stats.today().natural_breaks, 1);
    // a new stretch of work can earn another one
    tick_n(&mut c, 5, 0);
    tick_n(&mut c, 1, 150);
    assert_eq!(c.stats.today().natural_breaks, 2);
}

#[test]
fn no_natural_break_if_nothing_was_worked() {
    let mut c = core(test_settings());
    tick_n(&mut c, 5, 300);
    assert_eq!(c.stats.today().natural_breaks, 0);
}

#[test]
fn idle_reset_clears_snooze() {
    let mut c = core(test_settings());
    tick_n(&mut c, 10, 0);
    c.snooze(5);
    assert!(c.e.snooze_secs > 0);
    tick_n(&mut c, 1, 600);
    assert_eq!(c.e.snooze_secs, 0);
    assert_eq!(c.interval_secs(), 60);
}

fn start_break_with_difficulty(d: &str) -> Core {
    let mut s = test_settings();
    s.skip_difficulty = d.into();
    let mut c = core(s);
    tick_n(&mut c, 60, 0);
    assert!(c.e.brk.is_some());
    c
}

#[test]
fn casual_can_skip_immediately() {
    let mut c = start_break_with_difficulty("casual");
    let sn = c.snapshot();
    assert!(sn.can_skip);
    assert_eq!(sn.skip_in, 0);
    let fx = c.skip_or_end_break();
    assert_eq!(ends(&fx), 1);
    assert!(c.e.brk.is_none());
    assert_eq!(c.stats.today().breaks_skipped, 1);
    assert_eq!(taken(&mut c), 0);
}

#[test]
fn balanced_can_skip_only_after_delay() {
    let mut c = start_break_with_difficulty("balanced");
    let sn = c.snapshot();
    assert!(!sn.can_skip);
    assert_eq!(sn.skip_in, BALANCED_SKIP_DELAY);
    assert!(c.skip_or_end_break().is_empty());
    assert!(c.e.brk.is_some());

    tick_n(&mut c, BALANCED_SKIP_DELAY as u32 - 1, 0);
    assert!(!c.snapshot().can_skip);
    assert!(c.skip_or_end_break().is_empty());

    tick_n(&mut c, 1, 0);
    let sn = c.snapshot();
    assert!(sn.can_skip);
    assert_eq!(sn.skip_in, 0);
    assert_eq!(ends(&c.skip_or_end_break()), 1);
    assert_eq!(c.stats.today().breaks_skipped, 1);
}

#[test]
fn hardcore_cannot_skip() {
    let mut c = start_break_with_difficulty("hardcore");
    assert!(!c.snapshot().can_skip);
    tick_n(&mut c, 10, 0);
    assert!(!c.snapshot().can_skip);
    assert!(c.skip_or_end_break().is_empty());
    assert!(c.e.brk.is_some());
}

#[test]
fn hardcore_refuses_skip_next_and_snooze() {
    let mut s = test_settings();
    s.skip_difficulty = "hardcore".into();
    let mut c = core(s);
    tick_n(&mut c, 10, 0);
    assert!(c.skip_next().is_empty());
    assert!(c.snooze(5).is_empty());
    assert_eq!(c.e.snooze_secs, 0);
    assert_eq!(c.e.work_secs, 10);
}

#[test]
fn end_early_unlocks_at_seventy_percent() {
    // 20 s break -> 14 s
    let mut c = start_break_with_difficulty("hardcore");
    tick_n(&mut c, 13, 0);
    assert!(!c.snapshot().can_end);
    tick_n(&mut c, 1, 0);
    let sn = c.snapshot();
    assert!(sn.can_end);
    assert!(!sn.can_skip);
    let fx = c.skip_or_end_break();
    assert_eq!(ends(&fx), 1);
    // ending a mostly-done break counts as taken
    assert_eq!(taken(&mut c), 1);
    assert_eq!(c.stats.today().breaks_skipped, 0);
}

#[test]
fn end_early_respects_setting() {
    let mut s = test_settings();
    s.allow_end_early = false;
    s.skip_difficulty = "hardcore".into();
    let mut c = core(s);
    tick_n(&mut c, 60, 0);
    tick_n(&mut c, 18, 0);
    assert!(!c.snapshot().can_end);
    assert!(c.skip_or_end_break().is_empty());
}

#[test]
fn custom_break_can_always_end_but_counts_as_skipped_when_early() {
    let mut c = core(test_settings());
    let fx = c.take_break_now(Some(2));
    assert_eq!(starts(&fx), 1);
    let b = c.e.brk.as_ref().unwrap();
    assert_eq!(b.kind, "custom");
    assert_eq!(b.total_secs, 120);
    assert!(c.snapshot().can_end);
    assert_eq!(ends(&c.skip_or_end_break()), 1);
    assert_eq!(c.stats.today().breaks_skipped, 1);
    assert_eq!(taken(&mut c), 0);
}

#[test]
fn long_break_after_long_every_short_breaks() {
    let mut s = test_settings();
    s.long_enabled = true;
    s.long_every = 2;
    s.long_mins = 1;
    s.break_secs = 5;
    let mut c = core(s);
    for _ in 0..2 {
        assert!(!c.snapshot().next_is_long);
        assert_eq!(work_until_break(&mut c), "short");
        tick_n(&mut c, 5, 0);
        assert!(c.e.brk.is_none());
    }
    assert!(c.snapshot().next_is_long);
    assert_eq!(work_until_break(&mut c), "long");
    assert_eq!(c.e.brk.as_ref().unwrap().total_secs, 60);
    tick_n(&mut c, 60, 0);
    assert!(c.e.brk.is_none());
    assert_eq!(c.e.short_count, 0);
    assert_eq!(taken(&mut c), 3);
    // and the cycle starts over
    assert_eq!(work_until_break(&mut c), "short");
}

#[test]
fn no_long_breaks_when_disabled() {
    let mut s = test_settings();
    s.long_every = 1;
    s.break_secs = 5;
    let mut c = core(s);
    for _ in 0..4 {
        assert_eq!(work_until_break(&mut c), "short");
        tick_n(&mut c, 5, 0);
    }
}

#[test]
fn snooze_pushes_next_break_five_minutes_from_now() {
    let mut c = core(test_settings());
    tick_n(&mut c, 30, 0);
    c.snooze(5);
    assert_eq!(c.snapshot().next_break_in, 300);
    assert_eq!(starts(&tick_n(&mut c, 299, 0)), 0);
    assert!(c.e.brk.is_none());
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
}

#[test]
fn snooze_hides_heads_up_and_resets_after_the_break() {
    let mut c = core(test_settings());
    let fx = tick_n(&mut c, 55, 0);
    assert_eq!(shows(&fx), 1);
    let fx = c.snooze(5);
    assert_eq!(hides(&fx), 1);
    assert!(!c.e.prebreak_shown);
    // a fresh heads-up is shown before the snoozed break
    let fx = tick_n(&mut c, 300 - 5, 0);
    assert_eq!(shows(&fx), 1);
    assert_eq!(starts(&fx), 0);
    tick_n(&mut c, 5, 0);
    assert!(c.e.brk.is_some());
    tick_n(&mut c, 20, 0);
    assert_eq!(c.e.snooze_secs, 0);
    assert_eq!(c.snapshot().next_break_in, 60);
}

#[test]
fn skip_next_resets_countdown() {
    let mut c = core(test_settings());
    tick_n(&mut c, 55, 0);
    let fx = c.skip_next();
    assert_eq!(hides(&fx), 1);
    assert_eq!(c.e.work_secs, 0);
    assert_eq!(c.stats.today().breaks_skipped, 1);
    assert_eq!(starts(&tick_n(&mut c, 59, 0)), 0);
}

#[test]
fn wait_while_typing_delays_the_break() {
    let mut s = test_settings();
    s.wait_while_typing = true;
    let mut c = core(s);
    // typing continuously: held for up to MAX_TYPING_WAIT extra ticks
    assert_eq!(starts(&tick_n(&mut c, 60 + MAX_TYPING_WAIT - 1, 0)), 0);
    assert!(c.e.brk.is_none());
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
}

#[test]
fn wait_while_typing_starts_once_typing_stops() {
    let mut s = test_settings();
    s.wait_while_typing = true;
    let mut c = core(s);
    assert_eq!(starts(&tick_n(&mut c, 63, 0)), 0);
    // two seconds without input is enough
    assert_eq!(starts(&tick_n(&mut c, 1, 2)), 1);
}

#[test]
fn without_wait_while_typing_break_starts_on_time() {
    let mut c = core(test_settings());
    assert_eq!(starts(&tick_n(&mut c, 60, 0)), 1);
}

#[test]
fn smart_pause_holds_the_break() {
    let mut c = core(test_settings());
    let mut fx = Vec::new();
    for _ in 0..90 {
        fx.extend(c.tick(0, Some("In a meeting".into())));
    }
    assert_eq!(starts(&fx), 0);
    assert!(c.e.brk.is_none());
    let sn = c.snapshot();
    assert_eq!(sn.phase, "smart");
    assert_eq!(sn.reason.as_deref(), Some("In a meeting"));
    // the heads-up isn't shown while held
    assert_eq!(shows(&fx), 0);
}

#[test]
fn smart_pause_hides_an_already_visible_heads_up() {
    let mut c = core(test_settings());
    assert_eq!(shows(&tick_n(&mut c, 50, 0)), 1);
    let fx = c.tick(0, Some("Fullscreen app".into()));
    assert_eq!(hides(&fx), 1);
}

#[test]
fn smart_pause_waits_for_cooldown_after_reason_clears() {
    let mut s = test_settings();
    s.smart.cooldown_secs = 60;
    let mut c = core(s);
    tick_n(&mut c, 59, 0);
    // the break is due, but a meeting is on
    assert_eq!(starts(&c.tick(0, Some("In a meeting".into()))), 0);
    let now = now_epoch();
    assert!(c.e.smart_hold_until >= now + 59 && c.e.smart_hold_until <= now + 61);

    // reason gone, cooldown still running: keep holding
    let fx = tick_n(&mut c, 5, 0);
    assert_eq!(starts(&fx), 0);
    assert_eq!(c.snapshot().phase, "smart");

    // cooldown elapsed: the break goes ahead on the next tick
    c.e.smart_hold_until = now_epoch() - 1;
    let fx = tick_n(&mut c, 1, 0);
    assert_eq!(starts(&fx), 1);
    assert!(c.e.smart_reason.is_none());
}

#[test]
fn smart_pause_with_zero_cooldown_releases_immediately() {
    let mut s = test_settings();
    s.smart.cooldown_secs = 0;
    let mut c = core(s);
    tick_n(&mut c, 59, 0);
    assert_eq!(starts(&c.tick(0, Some("x".into()))), 0);
    assert_eq!(starts(&tick_n(&mut c, 1, 0)), 1);
}

fn hold_settings(max_hold_mins: u32) -> Settings {
    let mut s = test_settings();
    s.smart.cooldown_secs = 0;
    s.smart.max_hold_mins = max_hold_mins;
    s
}

fn tick_smart(c: &mut Core, n: usize) -> Vec<Effect> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(c.tick(0, Some("call".into())));
    }
    all
}

#[test]
fn smart_hold_below_limit_stays_held() {
    let mut c = core(hold_settings(1));
    tick_n(&mut c, 59, 0);
    // due at tick 60; held 59 of 60 seconds
    let fx = tick_smart(&mut c, 59);
    assert_eq!(starts(&fx), 0);
    assert_eq!(c.snapshot().phase, "smart");
    assert!(!c.snapshot().overdue);
    assert_eq!(c.snapshot().held_secs, 59);
}

#[test]
fn smart_hold_at_limit_shows_heads_up_then_starts_break() {
    let mut c = core(hold_settings(1));
    tick_n(&mut c, 59, 0);
    let fx = tick_smart(&mut c, 59);
    assert_eq!(starts(&fx), 0);
    // ceiling reached: heads-up appears, break not yet started
    let fx = tick_smart(&mut c, 1);
    assert_eq!(shows(&fx), 1);
    assert_eq!(starts(&fx), 0);
    assert!(c.snapshot().overdue);
    // the normal lead countdown runs, with the reason still active
    let fx = tick_smart(&mut c, 9);
    assert_eq!(starts(&fx), 0);
    let fx = tick_smart(&mut c, 1);
    assert_eq!(starts(&fx), 1);
    // everything resets once the break starts
    assert!(!c.snapshot().overdue);
    assert_eq!(c.snapshot().held_secs, 0);
}

#[test]
fn smart_hold_limit_zero_never_releases() {
    let mut c = core(hold_settings(0));
    tick_n(&mut c, 59, 0);
    assert_eq!(starts(&tick_smart(&mut c, 8000)), 0);
    assert!(!c.snapshot().overdue);
}

#[test]
fn smart_hold_counter_resets_after_reason_clears() {
    let mut c = core(hold_settings(1));
    tick_n(&mut c, 59, 0);
    tick_smart(&mut c, 30);
    assert_eq!(c.snapshot().held_secs, 30);
    // reason clears (zero cooldown) and the break goes ahead
    let fx = tick_n(&mut c, 1, 0);
    assert_eq!(starts(&fx), 1);
    assert_eq!(c.snapshot().held_secs, 0);
}

#[test]
fn smart_hold_counter_resets_when_reason_clears_without_a_break() {
    let mut s = hold_settings(1);
    s.wait_while_typing = true;
    let mut c = core(s);
    tick_n(&mut c, 59, 0);
    tick_smart(&mut c, 30);
    assert_eq!(c.snapshot().held_secs, 30);
    c.tick(0, None); // reason gone; the break starts or waits, but the hold is over
    assert_eq!(c.snapshot().held_secs, 0);
}

#[test]
fn smart_hold_counter_resets_on_snooze() {
    let mut c = core(hold_settings(1));
    tick_n(&mut c, 59, 0);
    tick_smart(&mut c, 30);
    c.snooze(5);
    assert_eq!(c.snapshot().held_secs, 0);
}

#[test]
fn manual_pause_stops_counting_until_resumed() {
    let mut c = core(test_settings());
    tick_n(&mut c, 30, 0);
    c.pause(None);
    assert_eq!(c.snapshot().phase, "paused");
    tick_n(&mut c, 100, 0);
    assert_eq!(c.e.work_secs, 30);
    c.resume();
    assert_eq!(starts(&tick_n(&mut c, 30, 0)), 1);
}

#[test]
fn start_now_begins_a_scheduled_break() {
    let mut c = core(test_settings());
    let fx = c.start_now();
    assert_eq!(starts(&fx), 1);
    assert_eq!(c.e.brk.as_ref().unwrap().kind, "short");
}

#[test]
fn manual_break_that_lasted_a_normal_break_resets_the_countdown() {
    let mut c = core(test_settings());
    tick_n(&mut c, 45, 0); // 45 of 60 s worked
    c.take_break_now(Some(5)); // a 5-minute manual break
    tick_n(&mut c, 25, 0); // longer than the normal 20 s break
    let fx = c.skip_or_end_break(); // end it early
    assert_eq!(ends(&fx), 1);
    assert_eq!(c.e.work_secs, 0, "a break you actually took restarts the countdown");
    assert_eq!(taken(&mut c), 1);
    // and the next heads-up is a full interval away, not seconds away
    assert_eq!(shows(&tick_n(&mut c, 30, 0)), 0);
}

#[test]
fn manual_break_ended_right_away_resumes_the_countdown() {
    let mut c = core(test_settings());
    tick_n(&mut c, 45, 0);
    c.take_break_now(Some(5));
    tick_n(&mut c, 3, 0); // ended almost immediately
    c.skip_or_end_break();
    assert_eq!(c.e.work_secs, 45, "a break abandoned after 3 s keeps the old countdown");
}

#[test]
fn snapshot_reports_holding_while_typing() {
    let mut s = test_settings();
    s.wait_while_typing = true;
    let mut c = core(s);
    tick_n(&mut c, 59, 5); // not typing
    assert!(!c.snapshot().holding);
    tick_n(&mut c, 3, 0); // due, but typing (idle 0)
    let snap = c.snapshot();
    assert!(snap.holding, "the UI should know the break is held for typing");
    assert!(snap.next_break_in <= 0);
    assert!(c.e.brk.is_none());
}
