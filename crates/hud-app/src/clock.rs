//! The system clock as Unix seconds.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current time in Unix seconds, `None` if the clock is before 1970.
pub fn unix_now() -> Option<i64> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_is_after_2025() {
        assert!(unix_now().is_some_and(|now| now > 1_735_689_600));
    }
}
