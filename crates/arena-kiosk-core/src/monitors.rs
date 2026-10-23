//! Where the kiosk window goes: the union of every monitor's rectangle, so one topmost window
//! covers all of them (recomputed at start and on every display change).

/// A rectangle in physical desktop pixels: its top-left corner and its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// From Win32's `left, top, right, bottom`.
    pub const fn from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self::new(left, top, right - left, bottom - top)
    }

    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }
}

/// The smallest rectangle covering every non-empty monitor, or `None` when there is none.
pub fn union(monitors: &[Rect]) -> Option<Rect> {
    let mut it = monitors.iter().filter(|r| r.width > 0 && r.height > 0);
    let first = it.next()?;
    let (mut l, mut t, mut r, mut b) = (
        i64::from(first.x),
        i64::from(first.y),
        first.right(),
        first.bottom(),
    );
    for m in it {
        l = l.min(i64::from(m.x));
        t = t.min(i64::from(m.y));
        r = r.max(m.right());
        b = b.max(m.bottom());
    }
    let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    Some(Rect::new(clamp(l), clamp(t), clamp(r - l), clamp(b - t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_monitors_side_by_side_offset_vertically() {
        let got = union(&[
            Rect::new(0, 0, 1920, 1080),
            Rect::new(1920, -200, 2560, 1440),
        ]);
        assert_eq!(got, Some(Rect::new(0, -200, 4480, 1440)));
    }

    #[test]
    fn one_monitor_is_itself_and_none_is_none() {
        let one = Rect::new(0, 0, 1366, 768);
        assert_eq!(union(&[one]), Some(one));
        assert_eq!(union(&[]), None);
        assert_eq!(union(&[Rect::new(5, 5, 0, 100)]), None);
    }

    #[test]
    fn a_monitor_left_of_the_primary_and_win32_edges() {
        let got = union(&[
            Rect::from_edges(0, 0, 1920, 1080),
            Rect::from_edges(-1280, 100, 0, 1124),
        ]);
        assert_eq!(got, Some(Rect::new(-1280, 0, 3200, 1124)));
    }
}
