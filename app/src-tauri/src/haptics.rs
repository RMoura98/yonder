//! Trackpad haptics during breaks (macOS Force Touch trackpads).
//! Other platforms get a no-op, and so do Macs without a Force Touch trackpad:
//! AppKit simply ignores the request there.

use crate::engine::Haptic;
use tauri::AppHandle;

/// Gap between the two pulses of the end-of-break cue.
#[cfg(target_os = "macos")]
const DOUBLE_PULSE_GAP: std::time::Duration = std::time::Duration::from_millis(140);

/// Play a break cue. Safe to call from any thread: AppKit is only touched on
/// the main thread.
#[cfg(target_os = "macos")]
pub fn play(app: &AppHandle, cue: Haptic) {
    match cue {
        Haptic::Start => on_main(app, mac::ALIGNMENT),
        Haptic::Tick => on_main(app, mac::LEVEL_CHANGE),
        Haptic::End => {
            on_main(app, mac::ALIGNMENT);
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(DOUBLE_PULSE_GAP);
                on_main(&app, mac::ALIGNMENT);
            });
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn play(_app: &AppHandle, _cue: Haptic) {}

#[cfg(target_os = "macos")]
fn on_main(app: &AppHandle, pattern: isize) {
    let _ = app.run_on_main_thread(move || mac::perform(pattern));
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    // NSHapticFeedbackPattern (NSInteger)
    pub const ALIGNMENT: isize = 1;
    pub const LEVEL_CHANGE: isize = 2;
    // NSHapticFeedbackPerformanceTime (NSUInteger)
    const PERFORMANCE_TIME_NOW: usize = 1;

    /// `[[NSHapticFeedbackManager defaultPerformer] performFeedbackPattern:performanceTime:]`.
    /// Must run on the main thread.
    pub fn perform(pattern: isize) {
        // Present since macOS 10.11; looked up rather than assumed.
        let Some(cls) = AnyClass::get(c"NSHapticFeedbackManager") else { return };
        objc2::rc::autoreleasepool(|_| unsafe {
            let performer: *mut AnyObject = msg_send![cls, defaultPerformer];
            if performer.is_null() {
                return;
            }
            let _: () = msg_send![performer, performFeedbackPattern: pattern, performanceTime: PERFORMANCE_TIME_NOW];
        });
    }
}
