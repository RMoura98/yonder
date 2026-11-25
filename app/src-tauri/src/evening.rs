//! Evening filter: the pure logic (schedule, colour temperature, ramps) and the small state
//! machine that decides what the display should look like each second. Nothing here talks to
//! the OS; `gamma.rs` applies what this module computes.

use crate::settings::Evening;
use std::time::{Duration, Instant};

/// The temperature that means "no tint".
pub const NEUTRAL_K: f64 = 6500.0;
/// How long the "Preview" button shows the filter at full strength.
pub const PREVIEW_SECS: u64 = 6;
/// A filter that is on is re-applied this often, because other software (a game, a driver
/// reset, waking the display) can reset the gamma ramp behind our back.
const REAPPLY_SECS: u64 = 30;

// ------------------------------------------------------------------ schedule

/// "HH:MM" -> minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

pub fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// How strong the filter should be (0.0 = off, 1.0 = full) at `now_min` minutes after midnight.
///
/// The evening runs from `from` to `to` (wrapping past midnight). The filter fades in over
/// `ramp` minutes after `from` and fades out over `ramp` minutes after `to`; the ramps are
/// shortened when the evening or the gap between evenings is shorter than `ramp`.
/// `from == to` means the schedule never runs.
pub fn schedule_strength(now_min: f64, from: u32, to: u32, ramp: u32) -> f64 {
    const DAY: f64 = 1440.0;
    if from == to {
        return 0.0;
    }
    let (from, to) = (from as f64, to as f64);
    let len = (to - from).rem_euclid(DAY);
    let since = (now_min - from).rem_euclid(DAY);
    let ramp = ramp as f64;
    if since < len {
        let r = ramp.min(len);
        return if r <= 0.0 { 1.0 } else { smoothstep(since / r) };
    }
    let r = ramp.min(DAY - len);
    let out = since - len;
    if r > 0.0 && out < r {
        1.0 - smoothstep(out / r)
    } else {
        0.0
    }
}

// ----------------------------------------------------------- colour mapping

/// Approximate blackbody colour of `kelvin` as RGB in 0..=1 (the widely published curve fit
/// by Tanner Helland, valid for roughly 1000K..40000K; the formula is public, this code is ours).
fn blackbody(kelvin: f64) -> [f64; 3] {
    let t = kelvin.clamp(1000.0, 40000.0) / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) };
    let g = if t <= 66.0 { 99.4708025861 * t.ln() - 161.1195681661 } else { 288.1221695283 * (t - 60.0).powf(-0.0755148492) };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    [r, g, b].map(|c| (c / 255.0).clamp(0.0, 1.0))
}

/// Per-channel multipliers (each 0..=1) that turn a 6500K display white into `kelvin`.
pub fn temperature_multipliers(kelvin: f64) -> [f64; 3] {
    let (w, c) = (blackbody(NEUTRAL_K), blackbody(kelvin.min(NEUTRAL_K)));
    [0, 1, 2].map(|i| (c[i] / w[i]).clamp(0.0, 1.0))
}

/// The multipliers for a filter at `strength` (0..=1): the tint and the dimming are both blended
/// in linearly, so strength 0 is exactly the untouched display. `floor` raises any channel that
/// would fall below it (Windows refuses gamma ramps that are too dark).
pub fn target_multipliers(strength: f64, kelvin: u32, dim_pct: u32, floor: f64) -> [f32; 3] {
    let s = strength.clamp(0.0, 1.0);
    let tint = temperature_multipliers(kelvin.clamp(1900, 6500) as f64);
    let dim = 1.0 - (dim_pct.min(60) as f64 / 100.0) * s;
    [0, 1, 2].map(|i| ((1.0 + (tint[i] - 1.0) * s) * dim).max(floor.min(1.0)).min(1.0) as f32)
}

/// A gamma ramp of `size` entries scaled by `mult` (0..=1), as 16-bit values.
pub fn ramp_table(mult: f32, size: usize) -> Vec<u16> {
    let m = mult.clamp(0.0, 1.0) as f64;
    (0..size)
        .map(|i| {
            let x = if size > 1 { i as f64 / (size - 1) as f64 } else { 0.0 };
            (x * m * 65535.0).round() as u16
        })
        .collect()
}

pub fn is_identity(m: &[f32; 3]) -> bool {
    m.iter().all(|v| *v >= 0.999)
}

// ---------------------------------------------------------------- controller

/// What the caller should do with the display this second.
#[derive(Debug, PartialEq)]
pub enum Step {
    Nothing,
    Apply([f32; 3]),
    /// Back to the normal, untouched display.
    Reset,
}

/// Decides the display state each second from the settings, the clock and the user's overrides.
#[derive(Default)]
pub struct Controller {
    /// Tray toggle: Some(true) forces the filter on, Some(false) forces it off, until the
    /// schedule next starts or ends. Never saved, so a restart always follows the schedule.
    manual: Option<bool>,
    preview_until: Option<Instant>,
    /// Whether the schedule wanted the filter on at the last step.
    sched_on: bool,
    /// What the display currently has applied (None = normal).
    applied: Option<[f32; 3]>,
    last_apply: Option<Instant>,
    /// Strength at the last step, for the UI and the tray.
    strength: f64,
}

impl Controller {
    /// Is the filter on (or fading) right now?
    pub fn active(&self) -> bool {
        self.strength > 0.0
    }

    pub fn strength(&self) -> f64 {
        self.strength
    }

    pub fn applied(&self) -> Option<[f32; 3]> {
        self.applied
    }

    /// The tray toggle: flips what you currently see.
    pub fn toggle_manual(&mut self) {
        self.manual = Some(!self.active());
    }

    pub fn start_preview(&mut self, now: Instant) {
        self.preview_until = Some(now + Duration::from_secs(PREVIEW_SECS));
    }

    /// The caller reports that applying failed, so the next step tries again.
    pub fn apply_failed(&mut self) {
        self.applied = None;
        self.last_apply = None;
    }

    /// The caller reports that it restored the display (for example on quit).
    pub fn mark_reset(&mut self) {
        self.applied = None;
        self.last_apply = None;
        self.strength = 0.0;
    }

    pub fn step(&mut self, cfg: &Evening, now_min: f64, now: Instant, floor: f64) -> Step {
        // the schedule
        let sched = match (cfg.enabled, parse_hhmm(&cfg.from), parse_hhmm(&cfg.to)) {
            (true, Some(f), Some(t)) => schedule_strength(now_min, f, t, cfg.transition_mins),
            _ => 0.0,
        };
        let sched_on = sched > 0.0;
        if sched_on != self.sched_on {
            self.sched_on = sched_on;
            self.manual = None; // a manual choice lasts until the schedule next changes
        }
        if self.preview_until.map_or(false, |t| now >= t) {
            self.preview_until = None;
        }
        self.strength = if self.preview_until.is_some() {
            1.0
        } else {
            match self.manual {
                Some(true) => 1.0,
                Some(false) => 0.0,
                None => sched,
            }
        };

        let target = target_multipliers(self.strength, cfg.temperature, cfg.dim, floor);
        if is_identity(&target) {
            return if self.applied.take().is_some() { Step::Reset } else { Step::Nothing };
        }
        let changed = self.applied.map_or(true, |a| a.iter().zip(&target).any(|(x, y)| (x - y).abs() > 0.002));
        let stale = self.last_apply.map_or(true, |t| now.duration_since(t) >= Duration::from_secs(REAPPLY_SECS));
        if changed || stale {
            self.applied = Some(target);
            self.last_apply = Some(now);
            Step::Apply(target)
        } else {
            Step::Nothing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f64 = 60.0;

    fn cfg() -> Evening {
        Evening { enabled: true, from: "21:00".into(), to: "07:00".into(), transition_mins: 30, temperature: 3400, dim: 10 }
    }

    #[test]
    fn parses_times() {
        assert_eq!(parse_hhmm("21:30"), Some(21 * 60 + 30));
        assert_eq!(parse_hhmm("0:05"), Some(5));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("21:60"), None);
        assert_eq!(parse_hhmm("soon"), None);
        assert_eq!(parse_hhmm(""), None);
    }

    #[test]
    fn schedule_wraps_past_midnight() {
        let (f, t) = (21 * 60, 7 * 60);
        assert_eq!(schedule_strength(12.0 * H, f, t, 30), 0.0);
        assert_eq!(schedule_strength(20.99 * H, f, t, 30), 0.0);
        assert_eq!(schedule_strength(21.0 * H, f, t, 30), 0.0); // the fade starts here
        assert_eq!(schedule_strength(22.0 * H, f, t, 30), 1.0);
        assert_eq!(schedule_strength(0.0, f, t, 30), 1.0);
        assert_eq!(schedule_strength(6.9 * H, f, t, 30), 1.0);
        assert_eq!(schedule_strength(7.0 * H + 30.0, f, t, 30), 0.0);
    }

    #[test]
    fn schedule_fades_smoothly_in_and_out() {
        let (f, t) = (21 * 60, 7 * 60);
        let mid_in = schedule_strength(21.0 * H + 15.0, f, t, 30);
        assert!((mid_in - 0.5).abs() < 1e-9);
        let mid_out = schedule_strength(7.0 * H + 15.0, f, t, 30);
        assert!((mid_out - 0.5).abs() < 1e-9);
        // monotonic on the way in
        let mut last = -1.0;
        for m in 0..=30 {
            let s = schedule_strength(21.0 * H + m as f64, f, t, 30);
            assert!(s >= last);
            last = s;
        }
        assert_eq!(last, 1.0);
    }

    #[test]
    fn schedule_same_day_and_degenerate() {
        let (f, t) = (9 * 60, 17 * 60);
        assert_eq!(schedule_strength(12.0 * H, f, t, 0), 1.0);
        assert_eq!(schedule_strength(8.0 * H, f, t, 0), 0.0);
        assert_eq!(schedule_strength(17.0 * H, f, t, 0), 0.0);
        assert_eq!(schedule_strength(12.0 * H, 600, 600, 30), 0.0); // from == to: never
        // an evening shorter than the ramp never gets the ramp longer than itself
        assert_eq!(schedule_strength(10.0 * H + 10.0, 600, 610, 30), 1.0);
        // evenings that nearly touch: the fade-out is cut to the gap and never exceeds 1
        let s = schedule_strength(7.0 * H + 1.0, 7 * 60 + 2, 7 * 60, 30);
        assert!((0.0..=1.0).contains(&s));
    }

    #[test]
    fn neutral_temperature_changes_nothing() {
        for m in temperature_multipliers(6500.0) {
            assert!((m - 1.0).abs() < 1e-9);
        }
        assert_eq!(target_multipliers(0.0, 2700, 50, 0.0), [1.0, 1.0, 1.0]);
        assert_eq!(target_multipliers(1.0, 6500, 0, 0.0), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn warmer_means_less_blue_than_green_than_red() {
        let mut prev = [1.0f64; 3];
        for k in [6000.0, 5000.0, 4000.0, 3400.0, 2700.0, 1900.0] {
            let m = temperature_multipliers(k);
            assert!(m[0] >= m[1] && m[1] >= m[2], "{k}: {m:?}");
            assert!(m[0] >= 0.99, "red stays full at {k}");
            for i in 0..3 {
                assert!(m[i] <= prev[i] + 1e-9, "{k} channel {i} should not brighten");
            }
            prev = m;
        }
        let w = temperature_multipliers(2700.0);
        assert!(w[2] < 0.5 && w[1] < 0.8, "{w:?}");
        let k3400 = temperature_multipliers(3400.0);
        assert!(k3400[2] > 0.45 && k3400[2] < 0.7, "{k3400:?}");
    }

    #[test]
    fn strength_blends_tint_and_dim() {
        let full = target_multipliers(1.0, 3400, 20, 0.0);
        let half = target_multipliers(0.5, 3400, 20, 0.0);
        let tint = temperature_multipliers(3400.0);
        assert!((full[2] as f64 - tint[2] * 0.8).abs() < 1e-6);
        assert!(half[2] > full[2] && half[2] < 1.0);
        // dimming alone leaves the colour balance alone
        let d = target_multipliers(1.0, 6500, 30, 0.0);
        assert!(d.iter().all(|v| (*v - 0.7).abs() < 1e-6));
        // dim is capped at 60%
        assert!((target_multipliers(1.0, 6500, 99, 0.0)[0] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn floor_keeps_channels_in_the_range_windows_accepts() {
        let m = target_multipliers(1.0, 1900, 60, 0.52);
        assert!(m.iter().all(|v| *v >= 0.52 - 1e-6));
        assert_eq!(target_multipliers(1.0, 1900, 60, 0.0)[2] < 0.3, true);
    }

    #[test]
    fn ramp_is_monotonic_and_scaled() {
        let r = ramp_table(1.0, 256);
        assert_eq!((r[0], r[255]), (0, 65535));
        assert!(r.windows(2).all(|w| w[0] <= w[1]));
        let h = ramp_table(0.5, 256);
        assert_eq!(h[0], 0);
        assert!((h[255] as i32 - 32768).abs() <= 1);
        assert_eq!(ramp_table(1.0, 1).len(), 1);
    }

    #[test]
    fn controller_follows_the_schedule_and_resets() {
        let mut c = Controller::default();
        let t0 = Instant::now();
        let cfg = cfg();
        assert_eq!(c.step(&cfg, 12.0 * H, t0, 0.0), Step::Nothing);
        let Step::Apply(m) = c.step(&cfg, 23.0 * H, t0, 0.0) else { panic!("should apply") };
        assert!(m[2] < 0.7);
        assert!(c.active());
        // unchanged: nothing to do until it goes stale
        assert_eq!(c.step(&cfg, 23.0 * H, t0 + Duration::from_secs(5), 0.0), Step::Nothing);
        assert!(matches!(c.step(&cfg, 23.0 * H, t0 + Duration::from_secs(31), 0.0), Step::Apply(_)));
        // morning: back to normal exactly once
        assert_eq!(c.step(&cfg, 9.0 * H, t0 + Duration::from_secs(40), 0.0), Step::Reset);
        assert_eq!(c.step(&cfg, 9.0 * H, t0 + Duration::from_secs(41), 0.0), Step::Nothing);
        assert!(!c.active());
    }

    #[test]
    fn disabled_schedule_does_nothing_but_the_toggle_still_works() {
        let mut c = Controller::default();
        let t0 = Instant::now();
        let mut cfg = cfg();
        cfg.enabled = false;
        assert_eq!(c.step(&cfg, 23.0 * H, t0, 0.0), Step::Nothing);
        c.toggle_manual();
        assert!(matches!(c.step(&cfg, 23.0 * H, t0, 0.0), Step::Apply(_)));
        c.toggle_manual();
        assert_eq!(c.step(&cfg, 23.0 * H, t0, 0.0), Step::Reset);
    }

    #[test]
    fn manual_choice_lasts_until_the_schedule_changes() {
        let mut c = Controller::default();
        let t0 = Instant::now();
        let cfg = cfg();
        c.step(&cfg, 23.0 * H, t0, 0.0);
        c.toggle_manual(); // switch it off for tonight
        assert_eq!(c.step(&cfg, 23.5 * H, t0, 0.0), Step::Reset);
        assert_eq!(c.step(&cfg, 3.0 * H, t0, 0.0), Step::Nothing);
        // the evening ends (schedule flips off), the override is dropped...
        assert_eq!(c.step(&cfg, 9.0 * H, t0, 0.0), Step::Nothing);
        // ...so the next evening starts normally
        assert!(matches!(c.step(&cfg, 22.0 * H, t0, 0.0), Step::Apply(_)));
    }

    #[test]
    fn preview_shows_full_strength_then_restores() {
        let mut c = Controller::default();
        let t0 = Instant::now();
        let mut cfg = cfg();
        cfg.enabled = false;
        c.start_preview(t0);
        let Step::Apply(m) = c.step(&cfg, 12.0 * H, t0, 0.0) else { panic!() };
        assert!(m[2] < 0.7);
        assert_eq!(c.step(&cfg, 12.0 * H, t0 + Duration::from_secs(PREVIEW_SECS + 1), 0.0), Step::Reset);
        assert!(c.applied().is_none());
    }

    #[test]
    fn a_failed_apply_is_retried() {
        let mut c = Controller::default();
        let t0 = Instant::now();
        let cfg = cfg();
        assert!(matches!(c.step(&cfg, 23.0 * H, t0, 0.0), Step::Apply(_)));
        c.apply_failed();
        assert!(matches!(c.step(&cfg, 23.0 * H, t0 + Duration::from_secs(1), 0.0), Step::Apply(_)));
    }
}
