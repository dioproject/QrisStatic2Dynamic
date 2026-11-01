use crate::db::DbPool;
use std::time::Duration;
use tracing::{error, info};

/// Retensi event notifikasi: 90 hari (RULES §13 DR-04).
const EVENT_RETENTION_DAYS: i64 = 90;
/// Retensi payload delivery: 90 hari (ERP §10).
const DELIVERY_RETENTION_DAYS: i64 = 90;
/// Jalankan retensi setiap N tick (dengan interval 30s → tiap 1 jam).
const RETENTION_EVERY_TICKS: u64 = 120;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

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

    /// Menjalankan background worker loop.
    pub async fn run(self) {
        info!(
            "Sweeper worker dimulai: interval = {:?}, grace = {}s",
            self.interval, self.grace_seconds
        );
        let mut interval_timer = tokio::time::interval(self.interval);
        let mut ticks: u64 = 0;

        loop {
            interval_timer.tick().await;
            ticks = ticks.wrapping_add(1);

            // 1. Sapu transaksi PENDING yang sudah melewati TTL + grace (TX-04)
            if let Err(e) = self.sweep_expired_transactions().await {
                error!("Gagal menyapu transaksi kedaluwarsa: {}", e);
            }

            // 2. Bersihkan request_nonces yang sudah berumur > 10 menit (DR-04)
            if let Err(e) = self.cleanup_old_nonces().await {
                error!("Gagal membersihkan nonces lama: {}", e);
            }

            // 3. Retensi berkala (DR-04/DR-08).
            if ticks.is_multiple_of(RETENTION_EVERY_TICKS)
                && let Err(e) = self.run_retention().await
            {
                error!("Gagal menjalankan retensi data: {}", e);
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

    /// Retensi data operasional (DR-04, DR-08).
    ///
    /// CATATAN PENTING: transaksi dan audit log **tidak** dihapus di sini.
    /// `AGENTS.md` §12.11 melarang hard delete transaksi/audit, sehingga
    /// penghapusan keduanya memerlukan keputusan pemilik proyek (mis. anonimisasi
    /// atau partisi arsip). Yang dibersihkan hanya data operasional turunan:
    /// event notifikasi dan payload delivery.
    pub async fn run_retention(&self) -> Result<(u64, u64), sqlx::Error> {
        let now = chrono::Utc::now().timestamp_millis();

        let events_cutoff = now - EVENT_RETENTION_DAYS * DAY_MS;
        let events_deleted = sqlx::query(
            r#"
            DELETE FROM webhook_events
            WHERE received_at < ?
            "#,
        )
        .bind(events_cutoff)
        .execute(&self.pool)
        .await?
        .rows_affected();

        let deliveries_cutoff = now - DELIVERY_RETENTION_DAYS * DAY_MS;
        let deliveries_deleted = sqlx::query(
            r#"
            DELETE FROM webhook_deliveries
            WHERE created_at < ?
              AND status IN ('DELIVERED','DEAD')
            "#,
        )
        .bind(deliveries_cutoff)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if events_deleted > 0 || deliveries_deleted > 0 {
            info!(
                "Retensi: {} event notifikasi dan {} delivery dihapus (>{}/{} hari)",
                events_deleted, deliveries_deleted, EVENT_RETENTION_DAYS, DELIVERY_RETENTION_DAYS
            );
        }

        Ok((events_deleted, deliveries_deleted))
    }
}
