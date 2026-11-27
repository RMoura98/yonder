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
fn firm_needs_the_hold_on_the_break_screen() {
    let mut c = start_break_with_difficulty("firm");
    let sn = c.snapshot();
    assert!(sn.can_skip);
    assert_eq!(sn.skip_in, 0);
    // Esc / tray / shortcut (a plain skip) do nothing while the break screen is up...
    assert!(c.skip_or_end_break().is_empty());
    assert!(c.skip_next().is_empty());
    assert!(c.e.brk.is_some());
    // ...only the held Skip button ends it, and it counts as skipped.
    assert_eq!(ends(&c.skip_held()), 1);
    assert!(c.e.brk.is_none());
    assert_eq!(c.stats.today().breaks_skipped, 1);
    assert_eq!(taken(&mut c), 0);
}

#[test]
fn firm_tray_skip_works_without_a_clickable_break_screen() {
    for style in ["gentle", "notify"] {
        let mut s = test_settings();
        s.skip_difficulty = "firm".into();
        s.break_style = style.into();
        let mut c = core(s);
        tick_n(&mut c, 60, 0);
        assert!(c.e.brk.is_some());
        assert_eq!(ends(&c.skip_or_end_break()), 1, "{style}");
    }
}

#[test]
fn firm_still_allows_skip_next_and_snooze_before_a_break() {
    let mut s = test_settings();
    s.skip_difficulty = "firm".into();
    let mut c = core(s);
    tick_n(&mut c, 10, 0);
    assert!(!c.skip_next().is_empty());
    assert_eq!(c.e.work_secs, 0);
}

#[test]
fn hold_cannot_bypass_hardcore() {
    let mut c = start_break_with_difficulty("hardcore");
    assert!(c.skip_held().is_empty());
    assert!(c.e.brk.is_some());
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
fn held_break_label_says_what_holds_it_and_when_it_starts() {
    let mut s = test_settings();
    s.language = "en".into();
    s.smart.cooldown_secs = 60;
    s.smart.max_hold_mins = 60;
    let mut c = core(s);
    // not due yet: the plain hold label
    c.tick(0, Some("you're in a call (Discord)".into()));
    assert_eq!(c.snapshot().label, "Holding breaks — you're in a call (Discord)");
    tick_n(&mut c, 58, 0);
    // due and held
    c.tick(0, Some("you're in a call (Discord)".into()));
    let l = c.snapshot().label;
    assert!(l.starts_with("Break due — waiting while you're in a call (Discord)."), "{l}");
    assert!(l.contains("It starts 1 minute after that."), "{l}");
    assert!(l.contains("At the latest in 60 minutes."), "{l}");
    // the call ended: only the cool-down is left
    c.tick(0, None);
    let l = c.snapshot().label;
    assert!(l.starts_with("Break in ") && l.ends_with("the hold is over"), "{l}");
}

#[test]
fn held_break_label_without_cooldown_or_limit() {
    let mut s = test_settings();
    s.language = "en".into();
    s.smart.cooldown_secs = 0;
    s.smart.max_hold_mins = 0;
    let mut c = core(s);
    tick_n(&mut c, 59, 0);
    c.tick(0, Some("Steam is open".into()));
    assert_eq!(c.snapshot().label, "Break due — waiting while Steam is open. It starts right after.");
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
fn overdue_hold_does_not_wait_for_typing() {
    let mut s = test_settings();
    s.wait_while_typing = true;
    s.smart.max_hold_mins = 1;
    s.prebreak_enabled = false;
    let mut c = core(s);
    tick_n(&mut c, 59, 0);
    // held by a game for the full minute, keys pressed all along (idle 0)
    let mut started = 0;
    for _ in 0..62 {
        started += starts(&c.tick(0, Some("a fullscreen app is running".into())));
    }
    assert!(c.e.hold_overdue || started > 0);
    for _ in 0..3 {
        started += starts(&c.tick(0, Some("a fullscreen app is running".into())));
    }
    assert_eq!(started, 1, "an overdue break starts at once, typing or not");
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

// ---------------------------------------------------------- restore (state.json)

fn saved_at(c: &Core, age: i64) -> SavedState {
    let mut s = c.saved_state();
    s.saved_at = now_epoch() - age;
    s
}

#[test]
fn restore_recent_snapshot_keeps_countdown() {
    let mut a = core(test_settings());
    tick_n(&mut a, 30, 0);
    a.e.short_count = 2;
    a.e.snooze_secs = 15;
    let saved = saved_at(&a, 20);
    let mut b = core(test_settings());
    assert!(b.restore(&saved, now_epoch()));
    assert_eq!(b.e.work_secs, 30);
    assert_eq!(b.e.snooze_secs, 15);
    assert_eq!(b.e.short_count, 2);
}

#[test]
fn restore_old_snapshot_starts_fresh() {
    let mut a = core(test_settings());
    tick_n(&mut a, 30, 0);
    let saved = saved_at(&a, RESTORE_MAX_AGE_SECS + 5);
    let mut b = core(test_settings());
    assert!(!b.restore(&saved, now_epoch()));
    assert_eq!(b.e.work_secs, 0);
}

#[test]
fn restore_gap_past_idle_threshold_is_a_natural_break() {
    let mut a = core(test_settings()); // idle_reset 2 min
    tick_n(&mut a, 30, 0);
    a.e.short_count = 1;
    let saved = saved_at(&a, 180);
    let mut b = core(test_settings());
    assert!(b.restore(&saved, now_epoch()));
    assert_eq!(b.e.work_secs, 0);
    assert_eq!(b.stats.today().natural_breaks, 1);
    assert_eq!(b.e.short_count, 1);
}

#[test]
fn restore_never_resumes_a_break() {
    let mut a = core(test_settings());
    work_until_break(&mut a);
    assert!(a.e.brk.is_some());
    let saved = saved_at(&a, 5);
    assert_eq!(saved.work_secs, 0);
    let mut b = core(test_settings());
    assert!(b.restore(&saved, now_epoch()));
    assert!(b.e.brk.is_none());
    assert_eq!(b.e.work_secs, 0);
}

#[test]
fn restore_drops_expired_pause_keeps_live_ones() {
    let mut a = core(test_settings());
    a.pause(Some(10));
    let mut saved = saved_at(&a, 5);
    saved.pause_until = Some(now_epoch() - 1);
    let mut b = core(test_settings());
    assert!(b.restore(&saved, now_epoch()));
    assert!(b.e.pause.is_none());

    let mut c = core(test_settings());
    assert!(c.restore(&saved_at(&a, 5), now_epoch()));
    assert!(matches!(c.e.pause, Some(Some(_))));

    a.pause(None);
    let mut d = core(test_settings());
    assert!(d.restore(&saved_at(&a, 5), now_epoch()));
    assert_eq!(d.e.pause, Some(None));
}

#[test]
fn corrupt_state_file_is_ignored() {
    let dir = std::env::temp_dir().join("yonder-test-state-corrupt");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("state.json");
    std::fs::write(&p, "{ not json").unwrap();
    assert!(SavedState::load(&p).is_none());
    assert!(SavedState::load(&dir.join("missing.json")).is_none());
    let mut a = core(test_settings());
    tick_n(&mut a, 5, 0);
    a.saved_state().save(&p);
    assert_eq!(SavedState::load(&p).unwrap().work_secs, 5);
    let _ = std::fs::remove_dir_all(&dir);
}

fn shown_reminders(fx: &[Effect]) -> usize {
    fx.iter().filter(|e| matches!(e, Effect::ShowReminders(_))).count()
}

#[test]
fn smart_pause_holds_wellness_reminders_until_it_ends() {
    let mut s = hold_settings(0);
    s.work_mins = 120;
    s.wellness.blink.enabled = true;
    s.wellness.blink.interval_mins = 1;
    let mut c = core(s);
    // Two blink intervals pass during a call: nothing is shown.
    assert_eq!(shown_reminders(&tick_smart(&mut c, 150)), 0);
    // The call ends: the held reminder shows once (not one per missed interval).
    assert_eq!(shown_reminders(&tick_n(&mut c, 1, 0)), 1);
    assert_eq!(shown_reminders(&tick_n(&mut c, 5, 0)), 0);
}
fn shown_ids(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::ShowReminders(l) => Some(l.iter().map(|r| r.id.clone()).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn reminders_due_together_show_the_one_that_waited_longest() {
    let mut s = test_settings();
    s.work_mins = 120;
    s.wellness.posture.enabled = true;
    s.wellness.posture.interval_mins = 2;
    s.wellness.blink.enabled = true;
    s.wellness.blink.interval_mins = 1;
    let mut c = core(s);
    // minute 1: blink alone
    assert_eq!(shown_ids(&tick_n(&mut c, 60, 0)), ["blink"]);
    // minute 2: both due; blink showed a minute ago, so only posture appears
    assert_eq!(shown_ids(&tick_n(&mut c, 60, 0)), ["posture"]);
    // minute 3: blink alone again, then minute 4 is posture's turn once more
    assert_eq!(shown_ids(&tick_n(&mut c, 60, 0)), ["blink"]);
    assert_eq!(shown_ids(&tick_n(&mut c, 60, 0)), ["posture"]);
}

#[test]
fn sticky_reminders_are_never_skipped() {
    let mut s = test_settings();
    s.work_mins = 120;
    s.wellness.posture.enabled = true;
    s.wellness.posture.interval_mins = 1;
    s.wellness.blink.enabled = true;
    s.wellness.blink.interval_mins = 1;
    s.wellness.blink.sticky = true;
    let mut c = core(s);
    let mut ids = shown_ids(&tick_n(&mut c, 60, 0));
    ids.sort();
    assert_eq!(ids, ["blink", "posture"]);
}

// ---- what happens when a break is due behind a fullscreen app

fn fs_core(action: &str) -> Core {
    let mut s = hold_settings(60);
    s.smart.fullscreen_action = action.into();
    core(s)
}

fn tick_fs(c: &mut Core, n: usize) -> Vec<Effect> {
    let mut all = Vec::new();
    for _ in 0..n {
        c.e.smart_fullscreen = true;
        all.extend(c.tick(0, Some("a fullscreen app is running".into())));
    }
    all
}

fn notices(fx: &[Effect]) -> usize {
    count(fx, |e| matches!(e, Effect::NotifyDue { .. }))
}

#[test]
fn fullscreen_wait_holds_the_break() {
    let mut c = fs_core("wait");
    tick_n(&mut c, 59, 0);
    let fx = tick_fs(&mut c, 30);
    assert_eq!(starts(&fx), 0);
    assert_eq!(notices(&fx), 0);
    assert_eq!(c.snapshot().phase, "smart");
}

#[test]
fn fullscreen_unknown_action_behaves_like_wait() {
    let mut c = fs_core("nonsense");
    tick_n(&mut c, 59, 0);
    let fx = tick_fs(&mut c, 30);
    assert_eq!(starts(&fx), 0);
    assert_eq!(notices(&fx), 0);
}

#[test]
fn fullscreen_show_starts_the_break_anyway() {
    let mut c = fs_core("show");
    tick_n(&mut c, 59, 0);
    let fx = tick_fs(&mut c, 1);
    assert_eq!(starts(&fx), 1);
}

#[test]
fn fullscreen_notify_sends_one_notice_and_no_break() {
    let mut c = fs_core("notify");
    tick_n(&mut c, 59, 0);
    let fx = tick_fs(&mut c, 1);
    assert_eq!(notices(&fx), 1);
    assert_eq!(starts(&fx), 0);
    assert!(c.e.brk.is_none());
    // the countdown restarted: nothing more for a whole interval
    let fx = tick_fs(&mut c, 58);
    assert_eq!(notices(&fx), 0);
    assert_eq!(starts(&fx), 0);
    // counted as a skipped break, and the cycle moved on
    assert_eq!(c.stats.today().breaks_skipped, 1);
    assert_eq!(c.stats.today().breaks_taken, 0);
    assert_eq!(c.e.short_count, 1);
    // the next one is due a minute later
    let fx = tick_fs(&mut c, 2);
    assert_eq!(notices(&fx), 1);
}

#[test]
fn fullscreen_skip_drops_the_break_silently() {
    let mut c = fs_core("skip");
    tick_n(&mut c, 59, 0);
    let fx = tick_fs(&mut c, 1);
    assert_eq!(notices(&fx), 0);
    assert_eq!(starts(&fx), 0);
    assert!(c.e.brk.is_none());
    assert_eq!(c.stats.today().breaks_skipped, 1);
    assert_eq!(c.e.work_secs, 0);
}

#[test]
fn fullscreen_choice_ignored_for_other_reasons() {
    for action in ["notify", "skip", "show"] {
        let mut c = fs_core(action);
        tick_n(&mut c, 59, 0);
        // a call (not flagged as fullscreen) still holds the break
        let fx = tick_smart(&mut c, 30);
        assert_eq!(starts(&fx), 0, "{action}");
        assert_eq!(notices(&fx), 0, "{action}");
        assert_eq!(c.snapshot().phase, "smart");
    }
}

#[test]
fn fullscreen_notify_waits_until_due() {
    let mut c = fs_core("notify");
    tick_n(&mut c, 30, 0);
    let fx = tick_fs(&mut c, 20);
    assert_eq!(notices(&fx), 0);
    assert_eq!(c.stats.today().breaks_skipped, 0);
}

#[test]
fn settings_without_fullscreen_action_default_to_wait() {
    let s: Settings = serde_json::from_str(r#"{"smart":{"fullscreen":true}}"#).unwrap();
    assert_eq!(s.smart.fullscreen_action, "wait");
}

// ------------------------------------------------------------- haptics

fn haptics(fx: &[Effect], cue: Haptic) -> usize {
    count(fx, |e| matches!(e, Effect::Haptic(h) if *h == cue))
}

#[test]
fn haptic_ticks_on_the_last_three_seconds_only() {
    assert!(!haptic_tick_due(0));
    assert!(haptic_tick_due(1));
    assert!(haptic_tick_due(2));
    assert!(haptic_tick_due(3));
    assert!(!haptic_tick_due(4));
    assert!(!haptic_tick_due(20));
}

#[test]
fn haptics_are_off_by_default() {
    assert!(!Settings::default().haptics);
    let mut c = core(test_settings());
    let fx = tick_n(&mut c, 80, 0);
    assert_eq!(ends(&fx), 1);
    assert_eq!(count(&fx, |e| matches!(e, Effect::Haptic(_))), 0);
}

#[test]
fn haptics_follow_the_break() {
    let mut s = test_settings();
    s.haptics = true;
    let mut c = core(s);
    let fx = tick_n(&mut c, 60, 0);
    assert_eq!(starts(&fx), 1);
    assert_eq!(haptics(&fx, Haptic::Start), 1);
    // 20 s break: nothing until 3 s are left
    let fx = tick_n(&mut c, 16, 0);
    assert_eq!(count(&fx, |e| matches!(e, Effect::Haptic(_))), 0);
    // remaining 3, 2, 1: one tick each
    for left in [3u64, 2, 1] {
        let fx = tick_n(&mut c, 1, 0);
        assert_eq!(c.e.brk.as_ref().unwrap().remaining(), left);
        assert_eq!(haptics(&fx, Haptic::Tick), 1);
    }
    // the end gets its own cue, not a fourth tick
    let fx = tick_n(&mut c, 1, 0);
    assert_eq!(ends(&fx), 1);
    assert_eq!(haptics(&fx, Haptic::End), 1);
    assert_eq!(haptics(&fx, Haptic::Tick), 0);
}

#[test]
fn skipping_a_break_still_pulses_the_end() {
    let mut s = test_settings();
    s.haptics = true;
    let mut c = core(s);
    tick_n(&mut c, 60, 0);
    tick_n(&mut c, 10, 0); // past the balanced skip delay
    let fx = c.skip_or_end_break();
    assert_eq!(ends(&fx), 1);
    assert_eq!(haptics(&fx, Haptic::End), 1);
}