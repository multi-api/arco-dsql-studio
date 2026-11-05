//! DSQL's optimistic concurrency: a transaction that loses a conflict fails, usually at COMMIT, with SQLSTATE 40001
//! (`OC000` for data, `OC001` for schema in the message), and the whole transaction should simply be run again.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use crate::error::CoreError;

pub fn is_occ(err: &CoreError) -> bool {
    matches!(err, CoreError::Db { sqlstate: Some(s), .. } if s == "40001")
}

/// Wait before retry number `attempt` (0-based): a random 50 ms up to 100, 200, then 400 ms, so clients that collided
/// do not collide again in lockstep.
pub fn backoff(attempt: u32) -> Duration {
    let cap = (100u64 << attempt.min(2)).min(400);
    let jitter = RandomState::new().build_hasher().finish() % (cap - 50 + 1);
    Duration::from_millis(50 + jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(sqlstate: Option<&str>) -> CoreError {
        CoreError::Db {
            sqlstate: sqlstate.map(str::to_string),
            message: "change conflicts with another transaction (OC000)".to_string(),
            detail: None,
            hint: None,
        }
    }

    #[test]
    fn occ_detects_40001() {
        assert!(is_occ(&db(Some("40001"))));
        assert!(!is_occ(&db(Some("42501"))));
        assert!(!is_occ(&db(None)));
        assert!(!is_occ(&CoreError::Cancelled));
    }

    #[test]
    fn backoff_stays_between_50_and_400_ms() {
        for attempt in 0..10 {
            for _ in 0..50 {
                let ms = backoff(attempt).as_millis();
                assert!((50..=400).contains(&ms), "attempt {attempt}: {ms} ms");
                if attempt == 0 {
                    assert!(ms <= 100);
                }
            }
        }
    }
}
