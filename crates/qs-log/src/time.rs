// SPDX-License-Identifier: GPL-3.0-or-later

//! UTC timestamps for log lines and crash-record file names, without a date crate.

use std::time::{SystemTime, UNIX_EPOCH};

/// The last second of 9999-12-31, so every year prints as four digits.
const MAX_SECS: u64 = 253_402_300_799;

const SECS_PER_DAY: u64 = 86_400;

/// A moment broken into UTC calendar fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Utc {
    year: u64,
    month: u64,
    day: u64,
    hour: u64,
    minute: u64,
    second: u64,
    millis: u32,
}

impl Utc {
    /// Breaks `time` into UTC fields. Moments before 1970 print as the epoch and
    /// moments after 9999 as its last second.
    pub(crate) fn from_system_time(time: SystemTime) -> Self {
        let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
        let secs = since.as_secs().min(MAX_SECS);
        let millis = since.subsec_millis();
        let (days, rest) = (secs.div_euclid(SECS_PER_DAY), secs.rem_euclid(SECS_PER_DAY));
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: rest.div_euclid(3_600),
            minute: rest.rem_euclid(3_600).div_euclid(60),
            second: rest.rem_euclid(60),
            millis,
        }
    }

    /// RFC 3339 with milliseconds, as written at the start of each log line:
    /// `2026-10-16T12:34:56.789Z`.
    pub(crate) fn rfc3339(self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.millis
        )
    }

    /// A compact form with no characters that Windows forbids in file names:
    /// `20261016T123456.789Z`.
    pub(crate) fn compact(self) -> String {
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}.{:03}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.millis
        )
    }
}

/// Howard Hinnant's `civil_from_days` for days since 1970-01-01, which is never
/// negative here. Returns the year, month (1 to 12) and day (1 to 31).
#[expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "days is at most MAX_SECS / 86400, so no step can overflow or underflow"
)]
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::arithmetic_side_effects,
        reason = "test code: small fixed offsets from the epoch"
    )]

    use std::time::Duration;

    use super::*;

    fn at(secs: u64, millis: u64) -> Utc {
        Utc::from_system_time(
            UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(millis),
        )
    }

    #[test]
    fn epoch() {
        assert_eq!(at(0, 0).rfc3339(), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn known_moments() {
        assert_eq!(at(951_782_400, 5).rfc3339(), "2000-02-29T00:00:00.005Z");
        assert_eq!(at(1_709_251_199, 999).rfc3339(), "2024-02-29T23:59:59.999Z");
        assert_eq!(at(1_791_000_000, 0).rfc3339(), "2026-10-03T04:00:00.000Z");
        assert_eq!(at(4_107_542_400, 0).rfc3339(), "2100-03-01T00:00:00.000Z");
    }

    #[test]
    fn clamps_before_epoch_and_after_9999() {
        let before = UNIX_EPOCH - Duration::from_secs(10);
        assert_eq!(
            Utc::from_system_time(before).rfc3339(),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(at(MAX_SECS, 0).rfc3339(), "9999-12-31T23:59:59.000Z");
        assert_eq!(
            at(MAX_SECS + 1_000_000, 0).rfc3339(),
            "9999-12-31T23:59:59.000Z"
        );
    }

    #[test]
    fn compact_has_no_colons() {
        assert_eq!(at(951_782_400, 5).compact(), "20000229T000000.005Z");
    }
}
