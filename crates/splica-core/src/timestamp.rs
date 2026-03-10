//! Rational timestamp with timebase, arithmetic operations, and overflow protection.

use std::fmt;

/// A rational timestamp representing a point in time within a media stream.
///
/// Internally stores `ticks` (numerator) and `timebase` (ticks per second).
/// For example, frame 150 at 30fps is `Timestamp { ticks: 150, timebase: 30 }` = 5.0 seconds.
///
/// All arithmetic operations use checked math and return `Option` to prevent silent overflow.
#[derive(Clone, Copy)]
#[must_use]
pub struct Timestamp {
    ticks: i64,
    timebase: u32,
}

impl Timestamp {
    /// Creates a new timestamp.
    ///
    /// # Panics
    ///
    /// Panics if `timebase` is zero.
    pub fn new(ticks: i64, timebase: u32) -> Self {
        assert!(timebase > 0, "timebase must be non-zero");
        Self { ticks, timebase }
    }

    /// Creates a timestamp representing zero in the given timebase.
    pub fn zero(timebase: u32) -> Self {
        Self::new(0, timebase)
    }

    /// Creates a timestamp from seconds and a target timebase.
    ///
    /// Returns `None` if the conversion would overflow.
    pub fn from_seconds(seconds: f64, timebase: u32) -> Option<Self> {
        assert!(timebase > 0, "timebase must be non-zero");
        let ticks = seconds * f64::from(timebase);
        if ticks > i64::MAX as f64 || ticks < i64::MIN as f64 {
            return None;
        }
        Some(Self {
            ticks: ticks.round() as i64,
            timebase,
        })
    }

    /// Returns the tick count (numerator).
    pub fn ticks(self) -> i64 {
        self.ticks
    }

    /// Returns the timebase (ticks per second).
    pub fn timebase(self) -> u32 {
        self.timebase
    }

    /// Converts to seconds as a floating-point value.
    pub fn as_seconds_f64(self) -> f64 {
        self.ticks as f64 / f64::from(self.timebase)
    }

    /// Rescales this timestamp to a different timebase.
    ///
    /// Returns `None` if the conversion would overflow.
    pub fn rescale(self, target_timebase: u32) -> Option<Self> {
        assert!(target_timebase > 0, "target timebase must be non-zero");
        if self.timebase == target_timebase {
            return Some(self);
        }
        // ticks * target_timebase / self.timebase
        let numerator = (self.ticks as i128) * (target_timebase as i128);
        let result = numerator / (self.timebase as i128);
        i64::try_from(result).ok().map(|ticks| Self {
            ticks,
            timebase: target_timebase,
        })
    }

    /// Checked addition. Returns `None` on overflow or timebase mismatch.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        if self.timebase != other.timebase {
            // Rescale other to our timebase, then add
            let other = other.rescale(self.timebase)?;
            self.ticks.checked_add(other.ticks).map(|ticks| Self {
                ticks,
                timebase: self.timebase,
            })
        } else {
            self.ticks.checked_add(other.ticks).map(|ticks| Self {
                ticks,
                timebase: self.timebase,
            })
        }
    }

    /// Checked subtraction. Returns `None` on overflow or timebase mismatch.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        if self.timebase != other.timebase {
            let other = other.rescale(self.timebase)?;
            self.ticks.checked_sub(other.ticks).map(|ticks| Self {
                ticks,
                timebase: self.timebase,
            })
        } else {
            self.ticks.checked_sub(other.ticks).map(|ticks| Self {
                ticks,
                timebase: self.timebase,
            })
        }
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({}/{} = {:.6}s)", self.ticks, self.timebase, self.as_seconds_f64())
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_seconds = self.as_seconds_f64();
        let hours = (total_seconds / 3600.0) as u32;
        let minutes = ((total_seconds % 3600.0) / 60.0) as u32;
        let seconds = total_seconds % 60.0;
        write!(f, "{hours:02}:{minutes:02}:{seconds:06.3}")
    }
}

impl PartialEq for Timestamp {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Timestamp {}

impl std::hash::Hash for Timestamp {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Normalize to a canonical form for hashing: reduce ticks/timebase by GCD
        let g = gcd(self.ticks.unsigned_abs(), self.timebase as u64);
        let ticks = self.ticks / g as i64;
        let timebase = self.timebase / g as u32;
        ticks.hash(state);
        timebase.hash(state);
    }
}

impl PartialOrd for Timestamp {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Cross-multiply to compare without floating point:
        // self.ticks / self.timebase vs other.ticks / other.timebase
        // => self.ticks * other.timebase vs other.ticks * self.timebase
        let lhs = (self.ticks as i128) * (other.timebase as i128);
        let rhs = (other.ticks as i128) * (self.timebase as i128);
        lhs.cmp(&rhs)
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_that_timestamp_converts_to_seconds() {
        // GIVEN
        let ts = Timestamp::new(150, 30);

        // WHEN
        let seconds = ts.as_seconds_f64();

        // THEN
        assert!((seconds - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_that_timestamp_rescales_between_timebases() {
        // GIVEN — 5 seconds at 30fps
        let ts = Timestamp::new(150, 30);

        // WHEN — rescale to 48kHz audio timebase
        let rescaled = ts.rescale(48000).unwrap();

        // THEN — 5 seconds * 48000 = 240000 ticks
        assert_eq!(rescaled.ticks(), 240000);
        assert_eq!(rescaled.timebase(), 48000);
    }

    #[test]
    fn test_that_checked_add_returns_none_on_overflow() {
        // GIVEN
        let ts = Timestamp::new(i64::MAX, 1);

        // WHEN
        let result = ts.checked_add(Timestamp::new(1, 1));

        // THEN
        assert!(result.is_none());
    }

    #[test]
    fn test_that_timestamps_compare_across_timebases() {
        // GIVEN — 5 seconds in two different timebases
        let a = Timestamp::new(150, 30);
        let b = Timestamp::new(240000, 48000);

        // THEN
        assert_eq!(a, b);
    }

    #[test]
    fn test_that_timestamp_ordering_works() {
        // GIVEN
        let earlier = Timestamp::new(100, 30);
        let later = Timestamp::new(200, 30);

        // THEN
        assert!(earlier < later);
    }

    #[test]
    fn test_that_display_formats_as_timecode() {
        // GIVEN — 1 hour, 23 minutes, 45.678 seconds
        let ts = Timestamp::from_seconds(5025.678, 1000).unwrap();

        // WHEN
        let display = ts.to_string();

        // THEN
        assert_eq!(display, "01:23:45.678");
    }

    #[test]
    fn test_that_checked_add_works_across_timebases() {
        // GIVEN
        let a = Timestamp::new(30, 30); // 1 second
        let b = Timestamp::new(48000, 48000); // 1 second

        // WHEN
        let result = a.checked_add(b).unwrap();

        // THEN — 2 seconds at timebase 30
        assert_eq!(result.ticks(), 60);
        assert_eq!(result.timebase(), 30);
    }

    #[test]
    #[should_panic(expected = "timebase must be non-zero")]
    fn test_that_zero_timebase_panics() {
        let _ = Timestamp::new(0, 0);
    }
}
