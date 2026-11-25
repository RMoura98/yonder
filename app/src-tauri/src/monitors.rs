//! Pure monitor geometry: which screen holds a point, which captured display matches a
//! window's monitor, and what changed between two monitor lists. No OS calls, so it is
//! unit-tested here; lib.rs and platform.rs feed it real monitor rectangles.

/// A monitor rectangle: x, y (top-left, may be negative) and width, height.
pub type Rect = (i32, i32, u32, u32);

/// Whether a (physical) point lies inside the rectangle.
pub fn contains(r: Rect, px: f64, py: f64) -> bool {
    px >= r.0 as f64 && py >= r.1 as f64 && px < r.0 as f64 + r.2 as f64 && py < r.1 as f64 + r.3 as f64
}

/// Index of the rectangle holding the point.
pub fn index_at(rects: &[Rect], p: (f64, f64)) -> Option<usize> {
    rects.iter().position(|r| contains(*r, p.0, p.1))
}

fn overlap(a: Rect, b: Rect) -> i64 {
    let w = (a.0 as i64 + a.2 as i64).min(b.0 as i64 + b.2 as i64) - (a.0 as i64).max(b.0 as i64);
    let h = (a.1 as i64 + a.3 as i64).min(b.1 as i64 + b.3 as i64) - (a.1 as i64).max(b.1 as i64);
    if w > 0 && h > 0 {
        w * h
    } else {
        0
    }
}

/// The candidate display that covers `want` the most (ties: the nearest top-left corner).
/// `None` when nothing overlaps at all, so a monitor never gets another one's picture.
pub fn best_match(candidates: &[Rect], want: Rect) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (i, overlap(*c, want), (c.0 as i64 - want.0 as i64).abs() + (c.1 as i64 - want.1 as i64).abs()))
        .filter(|(_, o, _)| *o > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
        .map(|(i, _, _)| i)
}

/// Top-left for a floating window of `size` (physical) near the top of monitor `m`,
/// centered or right-aligned. `scale` is that monitor's DPI scale.
pub fn top_position(m: Rect, scale: f64, size: (u32, u32), align_right: bool) -> (i32, i32) {
    let top = m.1 + (48.0 * scale) as i32;
    let x = if align_right {
        m.0 + m.2 as i32 - size.0 as i32 - (20.0 * scale) as i32
    } else {
        m.0 + (m.2 as i32 - size.0 as i32) / 2
    };
    (x, top)
}

/// Converts a window's physical size on one monitor to the same logical size on another.
pub fn rescale(size: (u32, u32), from_scale: f64, to_scale: f64) -> (u32, u32) {
    if from_scale <= 0.0 || (from_scale - to_scale).abs() < 1e-6 {
        return size;
    }
    let k = to_scale / from_scale;
    ((size.0 as f64 * k).round() as u32, (size.1 as f64 * k).round() as u32)
}

/// What to do when the monitor list changes while windows are open: indices into `have`
/// to close (their monitor is gone) and indices into `now` to open (no window yet).
pub fn diff(have: &[Rect], now: &[Rect]) -> (Vec<usize>, Vec<usize>) {
    let close = (0..have.len()).filter(|&i| !now.contains(&have[i])).collect();
    let open = (0..now.len()).filter(|&i| !have.contains(&now[i])).collect();
    (close, open)
}

#[cfg(test)]
mod tests {
    use super::*;

    // a 1080p primary at the origin, a 4K at 150% to its left (negative x) and a
    // 1080p above it (negative y)
    const PRIMARY: Rect = (0, 0, 1920, 1080);
    const LEFT: Rect = (-3840, -500, 3840, 2160);
    const ABOVE: Rect = (0, -1080, 1920, 1080);

    #[test]
    fn point_picks_its_monitor_with_negative_coordinates() {
        let r = [PRIMARY, LEFT, ABOVE];
        assert_eq!(index_at(&r, (10.0, 10.0)), Some(0));
        assert_eq!(index_at(&r, (-1.0, 10.0)), Some(1));
        assert_eq!(index_at(&r, (-3840.0, -500.0)), Some(1));
        assert_eq!(index_at(&r, (100.0, -1.0)), Some(2));
        assert_eq!(index_at(&r, (1920.0, 0.0)), None); // right edge is exclusive
        assert_eq!(index_at(&r, (5000.0, 5000.0)), None);
    }

    #[test]
    fn capture_matches_the_monitor_by_overlap() {
        let caps = [ABOVE, PRIMARY, LEFT];
        assert_eq!(best_match(&caps, PRIMARY), Some(1));
        assert_eq!(best_match(&caps, LEFT), Some(2));
        assert_eq!(best_match(&caps, ABOVE), Some(0));
    }

    #[test]
    fn capture_tolerates_small_offsets_and_refuses_strangers() {
        let caps = [PRIMARY, LEFT];
        assert_eq!(best_match(&caps, (2, 1, 1918, 1079)), Some(0));
        // a monitor with no capture is not given another monitor's picture
        assert_eq!(best_match(&caps, (4000, 0, 1920, 1080)), None);
        assert_eq!(best_match(&[], PRIMARY), None);
        // touching but not overlapping
        assert_eq!(best_match(&[PRIMARY], (1920, 0, 1920, 1080)), None);
    }

    #[test]
    fn same_size_monitors_are_told_apart() {
        let caps = [(0, 0, 1920, 1080), (1920, 0, 1920, 1080)];
        assert_eq!(best_match(&caps, (1920, 0, 1920, 1080)), Some(1));
    }

    #[test]
    fn top_position_uses_the_target_monitor_and_its_scale() {
        // centered on the left 4K at 150%: 600px wide window
        assert_eq!(top_position(LEFT, 1.5, (600, 200), false), (-3840 + 1620, -500 + 72));
        assert_eq!(top_position(PRIMARY, 1.0, (400, 100), true), (1500, 48));
    }

    #[test]
    fn rescale_keeps_the_logical_size() {
        assert_eq!(rescale((300, 100), 1.0, 1.5), (450, 150));
        assert_eq!(rescale((450, 150), 1.5, 1.0), (300, 100));
        assert_eq!(rescale((300, 100), 1.0, 1.0), (300, 100));
        assert_eq!(rescale((300, 100), 0.0, 2.0), (300, 100));
    }

    #[test]
    fn diff_finds_unplugged_and_new_monitors() {
        assert_eq!(diff(&[PRIMARY, LEFT], &[PRIMARY, LEFT]), (vec![], vec![]));
        assert_eq!(diff(&[PRIMARY, LEFT], &[PRIMARY]), (vec![1], vec![]));
        assert_eq!(diff(&[PRIMARY], &[PRIMARY, ABOVE]), (vec![], vec![1]));
        // a resolution change is a close plus an open
        assert_eq!(diff(&[PRIMARY], &[(0, 0, 1280, 720)]), (vec![0], vec![0]));
    }
}
