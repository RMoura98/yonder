//! Idle time on Wayland compositors that implement `ext-idle-notify-v1`
//! (KWin, Sway, Hyprland, river, Wayfire, niri, ...). GNOME's Mutter does not,
//! and is handled over D-Bus in `platform.rs`.
//!
//! The protocol is event based: the compositor tells us when the seat has seen no
//! input for N ms ("idled") and when input comes back ("resumed"). A background
//! thread keeps one notification registered and records those events in an
//! [`IdleTracker`]; `idle_ms()` is then a cheap lock + subtraction, so the 1 s
//! tick never touches the socket. Written from the protocol description; the
//! swayidle approach (MIT) of one notification with a fixed timeout is the idea.

use std::time::{Duration, Instant};

/// How long without input before the compositor reports "idled". Idle time below
/// this reads as 0, which is far below anything Yonder acts on.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const TIMEOUT: Duration = Duration::from_secs(1);

/// Bookkeeping for idled / resumed events. Pure: the caller supplies the clock.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
struct IdleTracker {
    timeout: Duration,
    /// When the compositor told us the user went idle (already `timeout` after
    /// the last input).
    idled_at: Option<Instant>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl IdleTracker {
    fn new(timeout: Duration) -> Self {
        Self { timeout, idled_at: None }
    }

    fn on_idled(&mut self, now: Instant) {
        self.idled_at = Some(now);
    }

    fn on_resumed(&mut self) {
        self.idled_at = None;
    }

    /// Whole milliseconds since the last input, 0 while the user is active.
    fn idle_ms(&self, now: Instant) -> u64 {
        match self.idled_at {
            Some(at) => {
                let total = self.timeout + now.saturating_duration_since(at);
                total.as_millis().min(u64::MAX as u128) as u64
            }
            None => 0,
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::idle_ms;

#[cfg(target_os = "linux")]
mod linux {
    use super::{IdleTracker, TIMEOUT};
    use crate::logger;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::{Arc, Mutex, Once};
    use std::time::Instant;
    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::{wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols::ext::idle_notify::v1::client::{
        ext_idle_notification_v1::{self, ExtIdleNotificationV1},
        ext_idle_notifier_v1::ExtIdleNotifierV1,
    };

    const STARTING: u8 = 0;
    const READY: u8 = 1;
    const UNAVAILABLE: u8 = 2;

    static START: Once = Once::new();
    static STATUS: AtomicU8 = AtomicU8::new(STARTING);
    static TRACKER: Mutex<Option<Arc<Mutex<IdleTracker>>>> = Mutex::new(None);

    struct State {
        tracker: Arc<Mutex<IdleTracker>>,
    }

    /// Milliseconds since the last input, or `None` when this backend can't tell
    /// (not started yet, no Wayland display, or the compositor lacks the protocol).
    /// The first call starts the background thread.
    pub fn idle_ms() -> Option<u64> {
        START.call_once(|| {
            let spawned = std::thread::Builder::new()
                .name("wayland-idle".into())
                .spawn(|| {
                    if let Err(e) = run() {
                        logger::info(&format!("wayland idle detection unavailable: {e}"));
                    }
                    STATUS.store(UNAVAILABLE, Ordering::Release);
                });
            if spawned.is_err() {
                STATUS.store(UNAVAILABLE, Ordering::Release);
            }
        });
        if STATUS.load(Ordering::Acquire) != READY {
            return None;
        }
        let tracker = TRACKER.lock().ok()?.clone()?;
        let ms = tracker.lock().ok()?.idle_ms(Instant::now());
        Some(ms)
    }

    /// Connects, registers the notification and dispatches events until the
    /// connection dies. Only returns on failure or disconnect.
    fn run() -> Result<(), String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
        let (globals, mut queue) =
            registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = queue.handle();

        let notifier: ExtIdleNotifierV1 = globals
            .bind(&qh, 1..=2, ())
            .map_err(|_| "compositor does not support ext-idle-notify-v1".to_string())?;
        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| "no wl_seat".to_string())?;

        let timeout_ms = TIMEOUT.as_millis() as u32;
        // Version 2 has a variant that ignores idle inhibitors (a playing video
        // must not make us think the user is still there, or the reverse).
        let _notification = if notifier.version() >= 2 {
            notifier.get_input_idle_notification(timeout_ms, &seat, &qh, ())
        } else {
            notifier.get_idle_notification(timeout_ms, &seat, &qh, ())
        };

        let tracker = Arc::new(Mutex::new(IdleTracker::new(TIMEOUT)));
        if let Ok(mut slot) = TRACKER.lock() {
            *slot = Some(tracker.clone());
        }
        let mut state = State { tracker };
        queue.roundtrip(&mut state).map_err(|e| format!("roundtrip: {e}"))?;
        STATUS.store(READY, Ordering::Release);
        logger::info("wayland idle detection: using ext-idle-notify-v1");

        loop {
            queue
                .blocking_dispatch(&mut state)
                .map_err(|e| format!("dispatch: {e}"))?;
        }
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<wl_seat::WlSeat, ()> for State {
        fn event(
            _: &mut Self,
            _: &wl_seat::WlSeat,
            _: wl_seat::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtIdleNotifierV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &ExtIdleNotifierV1,
            _: <ExtIdleNotifierV1 as Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtIdleNotificationV1, ()> for State {
        fn event(
            state: &mut Self,
            _: &ExtIdleNotificationV1,
            event: ext_idle_notification_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let Ok(mut t) = state.tracker.lock() {
                match event {
                    ext_idle_notification_v1::Event::Idled => t.on_idled(Instant::now()),
                    ext_idle_notification_v1::Event::Resumed => t.on_resumed(),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_until_idled() {
        let t0 = Instant::now();
        let t = IdleTracker::new(Duration::from_secs(1));
        assert_eq!(t.idle_ms(t0), 0);
        assert_eq!(t.idle_ms(t0 + Duration::from_secs(60)), 0);
    }

    #[test]
    fn counts_from_the_timeout_after_idled() {
        let t0 = Instant::now();
        let mut t = IdleTracker::new(Duration::from_secs(1));
        t.on_idled(t0);
        assert_eq!(t.idle_ms(t0), 1000);
        assert_eq!(t.idle_ms(t0 + Duration::from_millis(2500)), 3500);
    }

    #[test]
    fn resume_resets_to_zero() {
        let t0 = Instant::now();
        let mut t = IdleTracker::new(Duration::from_secs(1));
        t.on_idled(t0);
        t.on_resumed();
        assert_eq!(t.idle_ms(t0 + Duration::from_secs(5)), 0);
        // And a second idle period starts fresh.
        let t1 = t0 + Duration::from_secs(10);
        t.on_idled(t1);
        assert_eq!(t.idle_ms(t1 + Duration::from_secs(1)), 2000);
    }

    #[test]
    fn clock_going_backwards_does_not_panic() {
        let t0 = Instant::now() + Duration::from_secs(5);
        let mut t = IdleTracker::new(Duration::from_secs(1));
        t.on_idled(t0);
        assert_eq!(t.idle_ms(t0 - Duration::from_secs(2)), 1000);
    }
}
