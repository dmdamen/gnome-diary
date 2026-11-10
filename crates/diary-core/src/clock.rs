//! Where "now" comes from. The app uses [`SystemClock`]; tests use
//! [`ManualClock`] so dates and autosave timing are deterministic.

use chrono::{DateTime, FixedOffset};

pub trait Clock {
    fn now(&self) -> DateTime<FixedOffset>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<FixedOffset> {
        chrono::Local::now().fixed_offset()
    }
}

/// A clock that only moves when told to.
pub struct ManualClock {
    now: std::cell::Cell<DateTime<FixedOffset>>,
}

impl ManualClock {
    pub fn new(rfc3339: &str) -> Self {
        Self {
            now: std::cell::Cell::new(parse(rfc3339)),
        }
    }

    pub fn set(&self, rfc3339: &str) {
        self.now.set(parse(rfc3339));
    }

    pub fn advance(&self, by: chrono::Duration) {
        self.now.set(self.now.get() + by);
    }
}

fn parse(rfc3339: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(rfc3339).expect("invalid time")
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<FixedOffset> {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_is_close_to_real_time() {
        let diff = chrono::Utc::now().signed_duration_since(SystemClock.now());
        assert!(diff.num_seconds().abs() < 5);
    }

    #[test]
    fn manual_clock_starts_where_told() {
        let clock = ManualClock::new("2026-11-10T20:55:03+01:00");
        assert_eq!(clock.now().to_rfc3339(), "2026-11-10T20:55:03+01:00");
    }

    #[test]
    fn manual_clock_can_be_set_and_advanced() {
        let clock = ManualClock::new("2026-11-10T20:55:03+01:00");
        clock.advance(chrono::Duration::seconds(57));
        assert_eq!(clock.now().to_rfc3339(), "2026-11-10T20:56:00+01:00");
        clock.set("2027-01-01T00:00:00+00:00");
        assert_eq!(clock.now().to_rfc3339(), "2027-01-01T00:00:00+00:00");
    }

    #[test]
    #[should_panic(expected = "invalid time")]
    fn manual_clock_rejects_garbage() {
        ManualClock::new("yesterday");
    }
}
