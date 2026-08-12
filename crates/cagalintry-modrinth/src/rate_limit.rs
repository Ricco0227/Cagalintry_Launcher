//! Staying inside Modrinth's rate limit.
//!
//! Their documented allowance is 300 requests per minute per IP. Bursting is
//! fine — a search page fires several requests at once — so this is a sliding
//! window rather than a fixed delay between calls: requests go straight
//! through until 300 have happened inside a minute, and only then does the
//! next one wait for the oldest to age out.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::sync::Mutex;
// tokio's Instant, not std's: it moves with the runtime's clock, which is what
// makes the window's behaviour testable without spending real minutes, and
// keeps sleeping and measuring on the same timeline.
use tokio::time::Instant;

/// Modrinth's documented limit.
const LIMIT: usize = 300;
const WINDOW: Duration = Duration::from_secs(60);

/// Kept a little under the real limit. Their counter is per IP, and another
/// launcher or a browser on the same connection also spends from it.
const HEADROOM: usize = 20;

#[derive(Debug)]
pub struct RateLimiter {
    recent: Mutex<VecDeque<Instant>>,
    capacity: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(LIMIT - HEADROOM)
    }
}

impl RateLimiter {
    pub fn new(capacity: usize) -> Self {
        Self {
            recent: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity: capacity.max(1),
        }
    }

    /// Waits only if the window is full, then records this request.
    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut recent = self.recent.lock().await;
                let now = Instant::now();

                while let Some(oldest) = recent.front() {
                    if now.duration_since(*oldest) >= WINDOW {
                        recent.pop_front();
                    } else {
                        break;
                    }
                }

                if recent.len() < self.capacity {
                    recent.push_back(now);
                    return;
                }

                // Sleep only until the oldest request leaves the window.
                recent
                    .front()
                    .map(|oldest| WINDOW.saturating_sub(now.duration_since(*oldest)))
                    .unwrap_or_default()
            };

            // Lock released before sleeping, so other callers can still drain
            // expired entries and proceed.
            tokio::time::sleep(wait.max(Duration::from_millis(10))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requests_inside_the_window_are_not_delayed() {
        let limiter = RateLimiter::new(10);
        let started = Instant::now();
        for _ in 0..10 {
            limiter.acquire().await;
        }
        // Bursting is the normal case and must not be throttled.
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_window_blocks_until_the_oldest_expires() {
        let limiter = RateLimiter::new(2);
        limiter.acquire().await;
        limiter.acquire().await;

        // The third has to wait out the window. With time paused, tokio
        // auto-advances, so this asserts the wait happened rather than
        // spending a real minute.
        let started = Instant::now();
        limiter.acquire().await;
        assert!(started.elapsed() >= WINDOW - Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn capacity_frees_up_as_requests_age_out() {
        let limiter = RateLimiter::new(2);
        limiter.acquire().await;
        limiter.acquire().await;

        tokio::time::advance(WINDOW + Duration::from_secs(1)).await;

        // Both earlier requests have left the window, so this is immediate.
        let started = Instant::now();
        limiter.acquire().await;
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn the_default_leaves_headroom_under_modrinths_limit() {
        // Their counter is per IP; a browser on the same connection spends
        // from the same allowance.
        assert!(RateLimiter::default().capacity < LIMIT);
    }
}
