//! SMPTE 12M timecode representation with drop-frame support.
//!
//! Professional video workflows use HH:MM:SS:FF timecodes rather than
//! millisecond timestamps. Drop-frame timecodes (29.97fps) skip frame
//! numbers 00 and 01 at each minute boundary except every 10th minute
//! to keep the timecode display roughly synchronized with wall-clock time.

use std::fmt;

use crate::media::FrameRate;
use crate::timestamp::Timestamp;

/// A SMPTE 12M timecode with optional drop-frame counting.
///
/// Timecodes are displayed as `HH:MM:SS:FF` (non-drop-frame) or
/// `HH:MM:SS;FF` (drop-frame, using semicolon separator per SMPTE convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmpteTimecode {
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub frames: u32,
    pub drop_frame: bool,
}

impl SmpteTimecode {
    /// Converts a [`Timestamp`] to a SMPTE timecode at the given frame rate.
    ///
    /// For drop-frame mode, the frame rate must be 30000/1001 (29.97fps).
    /// The conversion computes the total frame count from the timestamp,
    /// then maps it to HH:MM:SS:FF using the SMPTE 12M drop-frame algorithm.
    pub fn from_timestamp(ts: Timestamp, rate: FrameRate, drop_frame: bool) -> Self {
        // Compute total frame count using i128 to avoid overflow:
        // frame_count = ticks * fps_num / (timebase * fps_den)
        let ticks = ts.ticks().max(0) as u128;
        let num = rate.numerator as u128;
        let den = rate.denominator as u128;
        let tb = ts.timebase() as u128;
        let frame_count = (ticks * num / (tb * den)) as u64;

        // Nominal integer frame rate (e.g., 30 for 30000/1001)
        let nominal_fps = rate.numerator.div_ceil(rate.denominator);

        if drop_frame {
            Self::from_frame_count_drop(frame_count, nominal_fps)
        } else {
            Self::from_frame_count_nondrop(frame_count, nominal_fps)
        }
    }

    /// Converts this timecode back to a [`Timestamp`].
    ///
    /// The resulting timestamp is at the start of the frame identified
    /// by this timecode. Round-trip accuracy is within one frame duration.
    pub fn to_timestamp(self, rate: FrameRate) -> Timestamp {
        let nominal_fps = rate.numerator.div_ceil(rate.denominator);
        let frame_count = if self.drop_frame {
            self.to_frame_count_drop(nominal_fps)
        } else {
            self.to_frame_count_nondrop(nominal_fps)
        };

        // timestamp = frame_count * fps_den / fps_num (in seconds)
        // In timebase = fps_num/fps_den: ticks = frame_count * fps_den
        // But we want a clean timebase. Use fps_num as timebase:
        // ticks = frame_count * fps_den, timebase = fps_num
        // This gives exact rational representation.
        let ticks = frame_count as i64 * rate.denominator as i64;
        // rate.numerator is guaranteed non-zero because FrameRate::new rejects zero denominator,
        // and numerator comes from a valid FrameRate that produces meaningful frame counts.
        Timestamp::new(ticks, rate.numerator).unwrap()
    }

    fn from_frame_count_nondrop(frame_count: u64, fps: u32) -> Self {
        let fps = fps as u64;
        let ff = (frame_count % fps) as u32;
        let total_seconds = frame_count / fps;
        let ss = (total_seconds % 60) as u32;
        let total_minutes = total_seconds / 60;
        let mm = (total_minutes % 60) as u32;
        let hh = (total_minutes / 60) as u32;
        Self {
            hours: hh,
            minutes: mm,
            seconds: ss,
            frames: ff,
            drop_frame: false,
        }
    }

    fn from_frame_count_drop(frame_count: u64, fps: u32) -> Self {
        let drop_frames: u64 = 2; // For 29.97fps; would be 4 for 59.94fps
        let fps = fps as u64;
        let frames_per_minute = fps * 60 - drop_frames; // 1798
        let frames_per_10min = frames_per_minute * 10 + drop_frames; // 17982

        let d = frame_count / frames_per_10min;
        let m = frame_count % frames_per_10min;

        let (minute_in_block, frame_in_minute) = if m < fps * 60 {
            // First minute of the 10-minute block (no drop)
            (0u64, m)
        } else {
            let m2 = m - fps * 60;
            (m2 / frames_per_minute + 1, m2 % frames_per_minute)
        };

        let total_minutes = d * 10 + minute_in_block;
        let hh = (total_minutes / 60) as u32;
        let mm = (total_minutes % 60) as u32;

        let (ss, ff) = if minute_in_block > 0 {
            // Dropped minute: frame numbers start at 2
            let adjusted = frame_in_minute + drop_frames;
            ((adjusted / fps) as u32, (adjusted % fps) as u32)
        } else {
            (
                (frame_in_minute / fps) as u32,
                (frame_in_minute % fps) as u32,
            )
        };

        Self {
            hours: hh,
            minutes: mm,
            seconds: ss,
            frames: ff,
            drop_frame: true,
        }
    }

    fn to_frame_count_nondrop(self, fps: u32) -> u64 {
        let fps = fps as u64;
        self.hours as u64 * 3600 * fps
            + self.minutes as u64 * 60 * fps
            + self.seconds as u64 * fps
            + self.frames as u64
    }

    fn to_frame_count_drop(self, fps: u32) -> u64 {
        let drop_frames: u64 = 2;
        let fps = fps as u64;
        let frames_per_minute = fps * 60 - drop_frames;
        let frames_per_10min = frames_per_minute * 10 + drop_frames;

        let total_minutes = self.hours as u64 * 60 + self.minutes as u64;
        let d = total_minutes / 10;
        let minute_in_block = total_minutes % 10;

        let frame_in_minute = if minute_in_block > 0 {
            // Dropped minute: subtract the 2 dropped frame numbers
            self.seconds as u64 * fps + self.frames as u64 - drop_frames
        } else {
            self.seconds as u64 * fps + self.frames as u64
        };

        d * frames_per_10min
            + if minute_in_block == 0 {
                frame_in_minute
            } else {
                fps * 60 + (minute_in_block - 1) * frames_per_minute + frame_in_minute
            }
    }
}

impl fmt::Display for SmpteTimecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sep = if self.drop_frame { ';' } else { ':' };
        write!(
            f,
            "{:02}:{:02}:{:02}{}{:02}",
            self.hours, self.minutes, self.seconds, sep, self.frames
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DF_RATE: FrameRate = FrameRate {
        numerator: 30000,
        denominator: 1001,
    };
    const NDF_RATE: FrameRate = FrameRate {
        numerator: 24,
        denominator: 1,
    };

    // Helper: create a timestamp from a frame count at 29.97fps
    // timebase = 30000, ticks = frame_count * 1001
    fn ts_from_frames_2997(frame_count: u64) -> Timestamp {
        Timestamp::new(frame_count as i64 * 1001, 30000).unwrap()
    }

    // -----------------------------------------------------------------------
    // Drop-frame display tests (SMPTE 12M compliance)
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_drop_frame_displays_zero() {
        // GIVEN — frame 0 at 29.97fps
        let tc = SmpteTimecode::from_timestamp(ts_from_frames_2997(0), DF_RATE, true);

        // THEN
        assert_eq!(tc.to_string(), "00:00:00;00");
    }

    #[test]
    fn test_that_drop_frame_displays_last_frame_before_first_drop() {
        // GIVEN — frame 1799 (last frame of minute 0)
        let tc = SmpteTimecode::from_timestamp(ts_from_frames_2997(1799), DF_RATE, true);

        // THEN
        assert_eq!(tc.to_string(), "00:00:59;29");
    }

    #[test]
    fn test_that_drop_frame_skips_frames_0_and_1_at_minute_1() {
        // GIVEN — frame 1800 (first frame of minute 1, frames 00 and 01 skipped)
        let tc = SmpteTimecode::from_timestamp(ts_from_frames_2997(1800), DF_RATE, true);

        // THEN — jumps from ;29 to ;02
        assert_eq!(tc.to_string(), "00:01:00;02");
    }

    #[test]
    fn test_that_drop_frame_does_not_skip_at_minute_10() {
        // GIVEN — frame 17982 (10-minute mark, no drop)
        let tc = SmpteTimecode::from_timestamp(ts_from_frames_2997(17982), DF_RATE, true);

        // THEN — minute 10 does NOT drop frames
        assert_eq!(tc.to_string(), "00:10:00;00");
    }

    #[test]
    fn test_that_drop_frame_displays_one_hour() {
        // GIVEN — frame 107892 (1 hour = 6 * 17982)
        let tc = SmpteTimecode::from_timestamp(ts_from_frames_2997(107892), DF_RATE, true);

        // THEN
        assert_eq!(tc.to_string(), "01:00:00;00");
    }

    // -----------------------------------------------------------------------
    // Non-drop-frame display test
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_non_drop_frame_uses_colon_separator() {
        // GIVEN — 90 seconds at 24fps = frame 2160
        let ts = Timestamp::new(2160, 24).unwrap();
        let tc = SmpteTimecode::from_timestamp(ts, NDF_RATE, false);

        // THEN
        assert_eq!(tc.to_string(), "00:01:30:00");
    }

    // -----------------------------------------------------------------------
    // Round-trip tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_drop_frame_round_trips_within_one_frame() {
        // GIVEN — several known frame counts
        let frame_duration_secs = 1001.0 / 30000.0;

        for &frame_count in &[0u64, 1799, 1800, 1801, 17982, 107892, 53946] {
            let original_ts = ts_from_frames_2997(frame_count);
            let tc = SmpteTimecode::from_timestamp(original_ts, DF_RATE, true);
            let round_tripped = tc.to_timestamp(DF_RATE);

            let diff = (original_ts.as_seconds_f64() - round_tripped.as_seconds_f64()).abs();
            assert!(
                diff < frame_duration_secs,
                "frame {frame_count}: diff {diff} >= frame duration {frame_duration_secs}, tc={tc}"
            );
        }
    }

    #[test]
    fn test_that_non_drop_frame_round_trips_within_one_frame() {
        // GIVEN — several frame counts at 24fps
        let frame_duration_secs = 1.0 / 24.0;

        for &frame_count in &[0u64, 23, 24, 1440, 86400] {
            let original_ts = Timestamp::new(frame_count as i64, 24).unwrap();
            let tc = SmpteTimecode::from_timestamp(original_ts, NDF_RATE, false);
            let round_tripped = tc.to_timestamp(NDF_RATE);

            let diff = (original_ts.as_seconds_f64() - round_tripped.as_seconds_f64()).abs();
            assert!(
                diff < frame_duration_secs,
                "frame {frame_count}: diff {diff} >= frame duration {frame_duration_secs}, tc={tc}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Verify Timestamp is unchanged
    // -----------------------------------------------------------------------

    #[test]
    fn test_that_timestamp_fields_are_unchanged() {
        // GIVEN — basic timestamp operations still work
        let ts = Timestamp::new(150, 30).unwrap();

        // THEN
        assert_eq!(ts.ticks(), 150);
        assert_eq!(ts.timebase(), 30);
        assert!((ts.as_seconds_f64() - 5.0).abs() < f64::EPSILON);
    }
}
