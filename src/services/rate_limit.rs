use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Batas jumlah key yang dilacak sebelum dilakukan pembersihan.
const MAX_TRACKED_KEYS: usize = 20_000;
/// Bucket yang tidak disentuh selama ini dianggap basi dan dibuang.
const STALE_AFTER: Duration = Duration::from_secs(600);

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitDecision {
    pub allowed: bool,
    /// Detik sampai satu token tersedia kembali (untuk header `Retry-After`).
    pub retry_after_secs: u64,
}

/// Token bucket in-memory dengan dua kelas limit: umum dan decode gambar
/// (RULES §9 RL-01 s/d RL-04).
///
/// Limit disimpan per key (kredensial atau IP), sehingga satu klien tidak
/// menghabiskan kuota klien lain. State hilang saat proses restart; untuk
/// deployment multi-instance, limit ini bersifat per-instance.
pub struct RateLimiter {
    per_minute: u32,
    burst: u32,
    image_per_minute: u32,
    image_burst: u32,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new(per_minute: u32, burst: u32, image_per_minute: u32) -> Self {
        Self {
            per_minute: per_minute.max(1),
            burst: burst.max(1),
            image_per_minute: image_per_minute.max(1),
            // Burst gambar tidak perlu lebih besar dari limit per menitnya.
            image_burst: image_per_minute.max(1),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Limit umum (RL-01/RL-02).
    pub fn check(&self, key: &str) -> RateLimitDecision {
        self.consume(key, self.per_minute, self.burst)
    }

    /// Limit decode gambar (RL-03).
    pub fn check_image(&self, key: &str) -> RateLimitDecision {
        self.consume(key, self.image_per_minute, self.image_burst)
    }

    fn consume(&self, key: &str, per_minute: u32, burst: u32) -> RateLimitDecision {
        let capacity = burst.max(1) as f64;
        let rate_per_sec = per_minute.max(1) as f64 / 60.0;

        let Ok(mut buckets) = self.buckets.lock() else {
            // Mutex poisoned: jangan memblokir layanan karena kegagalan internal.
            return RateLimitDecision {
                allowed: true,
                retry_after_secs: 0,
            };
        };

        if buckets.len() > MAX_TRACKED_KEYS {
            buckets.retain(|_, b| b.last_refill.elapsed() < STALE_AFTER);
        }

        let now = Instant::now();
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: capacity,
            last_refill: now,
        });

        // Refill proporsional terhadap waktu berjalan.
        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        if elapsed > 0.0 {
            bucket.tokens = (bucket.tokens + elapsed * rate_per_sec).min(capacity);
            bucket.last_refill = now;
        }

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            RateLimitDecision {
                allowed: true,
                retry_after_secs: 0,
            }
        } else {
            let deficit = 1.0 - bucket.tokens;
            let retry_after = (deficit / rate_per_sec).ceil() as u64;
            RateLimitDecision {
                allowed: false,
                retry_after_secs: retry_after.max(1),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_burst_then_reject() {
        let limiter = RateLimiter::new(60, 3, 20);

        // 3 permintaan pertama lolos sesuai burst.
        for _ in 0..3 {
            assert!(limiter.check("cred:a").allowed);
        }

        // Permintaan keempat harus ditolak.
        let denied = limiter.check("cred:a");
        assert!(!denied.allowed);
        assert!(denied.retry_after_secs >= 1);
    }

    #[test]
    fn test_keys_are_isolated() {
        let limiter = RateLimiter::new(60, 1, 20);

        assert!(limiter.check("cred:a").allowed);
        assert!(!limiter.check("cred:a").allowed);
        // Key lain tetap punya kuota penuh.
        assert!(limiter.check("cred:b").allowed);
    }

    #[test]
    fn test_image_limit_is_stricter() {
        let limiter = RateLimiter::new(120, 60, 1);

        assert!(limiter.check_image("img:a").allowed);
        assert!(!limiter.check_image("img:a").allowed);
        // Limit umum tidak terpengaruh.
        assert!(limiter.check("cred:a").allowed);
    }
}
