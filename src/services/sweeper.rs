use crate::db::DbPool;
use std::time::Duration;
use tracing::{error, info};

pub struct SweeperWorker {
    pool: DbPool,
    interval: Duration,
    grace_seconds: i64,
}

impl SweeperWorker {
    pub fn new(pool: DbPool, interval_seconds: u64, grace_seconds: i64) -> Self {
        Self {
            pool,
            interval: Duration::from_secs(interval_seconds),
            grace_seconds,
        }
    }

    /// Menjalankan background worker loop
    pub async fn run(self) {
        info!(
            "Sweeper worker dimulai: interval = {:?}, grace = {}s",
            self.interval, self.grace_seconds
        );
        let mut interval_timer = tokio::time::interval(self.interval);

        loop {
            interval_timer.tick().await;

            // 1. Sapu transaksi PENDING yang sudah melewati TTL + grace (TX-04)
            if let Err(e) = self.sweep_expired_transactions().await {
                error!("Gagal menyapu transaksi kedaluwarsa: {}", e);
            }

            // 2. Bersihkan request_nonces yang sudah berumur > 10 menit (DR-04)
            if let Err(e) = self.cleanup_old_nonces().await {
                error!("Gagal membersihkan nonces lama: {}", e);
            }
        }
    }

    pub async fn sweep_expired_transactions(&self) -> Result<u64, sqlx::Error> {
        let now = chrono::Utc::now().timestamp_millis();
        let grace_ms = self.grace_seconds * 1000;

        let rows = sqlx::query(
            r#"
            UPDATE transactions
            SET status = 'EXPIRED', updated_at = ?
            WHERE status = 'PENDING'
              AND (expires_at + ?) < ?
            "#,
        )
        .bind(now)
        .bind(grace_ms)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if rows > 0 {
            info!("Sweeper: {} transaksi PENDING diubah menjadi EXPIRED", rows);
        }

        Ok(rows)
    }

    pub async fn cleanup_old_nonces(&self) -> Result<u64, sqlx::Error> {
        let now = chrono::Utc::now().timestamp_millis();
        // 10 menit = 600.000 ms
        let ten_minutes_ago = now - 600_000;

        let rows = sqlx::query(
            r#"
            DELETE FROM request_nonces
            WHERE seen_at < ?
            "#,
        )
        .bind(ten_minutes_ago)
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(rows)
    }
}
