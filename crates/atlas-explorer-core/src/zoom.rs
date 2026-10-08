//! Icon size and row height (Ctrl+scroll, Ctrl+plus and minus, Ctrl+0), with
//! no Qt: the limits, the steps, and how wheel movement becomes steps. The
//! window keeps the two numbers (and saves them); this module only decides.

/// Icon size in the Icons view, in pixels.
pub const ICON_MIN: i32 = 48;
pub const ICON_MAX: i32 = 256;
pub const ICON_DEFAULT: i32 = 96;
pub const ICON_STEP: i32 = 16;

/// Row height in the Details and Compact views, in pixels.
pub const ROW_MIN: i32 = 24;
pub const ROW_MAX: i32 = 64;
pub const ROW_STEP: i32 = 4;

/// One notch of a mouse wheel, in the eighths of a degree Qt reports.
pub const WHEEL_NOTCH: i32 = 120;

/// Which size: the icons' or the rows'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Icon = 0,
    Row = 1,
}

impl Kind {
    pub fn from_code(code: u32) -> Option<Kind> {
        match code {
            0 => Some(Kind::Icon),
            1 => Some(Kind::Row),
            _ => None,
        }
    }
    fn limits(self) -> (i32, i32, i32) {
        match self {
            Kind::Icon => (ICON_MIN, ICON_MAX, ICON_STEP),
            Kind::Row => (ROW_MIN, ROW_MAX, ROW_STEP),
        }
    }
}

/// `value` brought into the limits of `kind`. Anything that was saved (a file
/// can hold any number) comes through here before it is used.
pub fn clamp(kind: Kind, value: i32) -> i32 {
    let (min, max, _) = kind.limits();
    value.clamp(min, max)
}

/// `current` moved by `steps` (negative: smaller), within the limits. A size
/// that is not on the step grid (a row height made of the font's size) lands on
/// the grid after its first step.
pub fn step(kind: Kind, current: i32, steps: i32) -> i32 {
    let (min, max, unit) = kind.limits();
    let current = current.clamp(min, max);
    if steps == 0 {
        return current;
    }
    let off = i64::from((current - min) % unit);
    let unit = i64::from(unit);
    let steps = i64::from(steps);
    // The first step goes to the next grid line (a whole unit when already on
    // one); the others are whole units.
    let moved = if steps > 0 {
        i64::from(current) + (unit - off) + (steps - 1) * unit
    } else {
        let back = if off == 0 { unit } else { off };
        i64::from(current) - back - (-steps - 1) * unit
    };
    moved.clamp(i64::from(min), i64::from(max)) as i32
}

/// The size shown when none is saved: the icons' default, and for rows
/// `default_row` (the caller passes what two grid units are), within limits.
pub fn default_size(kind: Kind, default_row: i32) -> i32 {
    match kind {
        Kind::Icon => ICON_DEFAULT,
        Kind::Row => clamp(Kind::Row, default_row),
    }
}

/// Adds a wheel movement of `delta` to the `pending` rest and returns the
/// whole steps it makes (a notch is one step; a touchpad's small movements
/// add up to one) and what is left over. Up (positive) is bigger.
pub fn wheel_steps(pending: i32, delta: i32) -> (i32, i32) {
    let total = pending.saturating_add(delta);
    (total / WHEEL_NOTCH, total % WHEEL_NOTCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_within_the_limits() {
        for kind in [Kind::Icon, Kind::Row] {
            let (min, max, _) = kind.limits();
            assert_eq!(step(kind, max, 5), max);
            assert_eq!(step(kind, min, -5), min);
            assert_eq!(step(kind, min, 1000), max);
            assert_eq!(step(kind, max, -1000), min);
            assert_eq!(step(kind, min, i32::MAX), max);
            assert_eq!(step(kind, max, i32::MIN + 1), min);
            // a saved number out of range is brought back first
            assert_eq!(step(kind, i32::MIN, 0), min);
            assert_eq!(step(kind, i32::MAX, 0), max);
            assert_eq!(clamp(kind, -7), min);
            assert_eq!(clamp(kind, 100_000), max);
        }
    }

    #[test]
    fn one_step_is_one_unit_and_back_again() {
        assert_eq!(step(Kind::Icon, 96, 1), 112);
        assert_eq!(step(Kind::Icon, 96, -1), 80);
        assert_eq!(step(Kind::Icon, 96, 3), 144);
        assert_eq!(step(Kind::Row, 36, 1), 40);
        assert_eq!(step(Kind::Row, 36, -2), 28);
        // up and down are inverses on the grid
        let mut v = ICON_DEFAULT;
        for _ in 0..4 {
            v = step(Kind::Icon, v, 1);
        }
        for _ in 0..4 {
            v = step(Kind::Icon, v, -1);
        }
        assert_eq!(v, ICON_DEFAULT);
    }

    #[test]
    fn a_size_off_the_grid_lands_on_it() {
        // 30 is between the 28 and 32 of the rows' grid (24, 28, 32, ...)
        assert_eq!(step(Kind::Row, 30, 1), 32);
        assert_eq!(step(Kind::Row, 30, -1), 28);
        assert_eq!(step(Kind::Row, 30, 2), 36);
        assert_eq!(step(Kind::Row, 30, -2), 24);
        assert_eq!(step(Kind::Icon, 100, 1), 112);
        assert_eq!(step(Kind::Icon, 100, -1), 96);
    }

    #[test]
    fn the_default_is_inside_the_limits() {
        assert_eq!(default_size(Kind::Icon, 0), ICON_DEFAULT);
        assert_eq!(default_size(Kind::Row, 36), 36);
        assert_eq!(default_size(Kind::Row, 2), ROW_MIN);
        assert_eq!(default_size(Kind::Row, 500), ROW_MAX);
        assert!((ICON_MIN..=ICON_MAX).contains(&ICON_DEFAULT));
    }

    #[test]
    fn wheel_movement_adds_up_to_steps() {
        assert_eq!(wheel_steps(0, 120), (1, 0));
        assert_eq!(wheel_steps(0, -120), (-1, 0));
        assert_eq!(wheel_steps(0, 360), (3, 0));
        // a touchpad: small movements
        let (mut rest, mut total) = (0, 0);
        for _ in 0..30 {
            let (s, r) = wheel_steps(rest, 10);
            total += s;
            rest = r;
        }
        assert_eq!((total, rest), (2, 60));
        // direction changes cancel the rest
        assert_eq!(wheel_steps(60, -60), (0, 0));
        assert_eq!(wheel_steps(60, -190), (-1, -10));
        assert!(wheel_steps(i32::MAX, i32::MAX).0 > 0);
        assert_eq!(Kind::from_code(2), None);
    }
}
