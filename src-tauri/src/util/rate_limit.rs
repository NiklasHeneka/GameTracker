use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

/// Spaces outbound requests to at most one per `interval`.
///
/// IGDB allows 4 requests/second and answers a burst with `429`. Rather than
/// reacting to rejections, callers reserve their slot up front: each waiter
/// claims the next free instant and releases the lock *before* sleeping, so
/// concurrent callers queue in arrival order instead of thundering.
pub struct RateLimiter {
    next_free: Mutex<Instant>,
    interval: Duration,
}

impl RateLimiter {
    pub fn per_second(rate: u32) -> Self {
        Self {
            next_free: Mutex::new(Instant::now()),
            interval: Duration::from_secs_f64(1.0 / rate.max(1) as f64),
        }
    }

    pub async fn acquire(&self) {
        let wait_until = {
            let mut next = self.next_free.lock().await;
            let now = Instant::now();
            let slot = (*next).max(now);
            *next = slot + self.interval;
            slot
        };

        let now = Instant::now();
        if wait_until > now {
            tokio::time::sleep(wait_until - now).await;
        }
    }
}
