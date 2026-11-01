use crate::auth::crypto::decrypt_secret;
use crate::db::DbPool;
use crate::error::AppError;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::time::Duration;
use tracing::{error, info, warn};

type HmacSha256 = Hmac<Sha256>;

/// Jadwal backoff retry (RULES §12 OW-04): 5s, 30s, 2m, 10m, 1h, 6h.
const BACKOFF_SECONDS: [i64; 6] = [5, 30, 120, 600, 3600, 21600];
const MAX_ATTEMPTS: i64 = 6;
const REQUEST_TIMEOUT_SECS: u64 = 10;

/// Validasi callback URL sebelum disimpan dan sebelum dikirim (RULES §9 RL-14, OW-12).
///
/// Aturan:
/// - wajib scheme `https`
/// - wajib punya host
/// - tolak loopback/private/link-local/multicast/reserved bila host berupa IP literal
/// - tolak hostname `localhost`, `*.localhost`, `*.local`, `*.internal`
///
/// `allow_insecure` HANYA untuk dev/test (`ALLOW_INSECURE_CALLBACKS=true`).
/// Validasi diulang saat pengiriman untuk mengurangi risiko DNS rebinding.
pub fn validate_callback_url(raw_url: &str, allow_insecure: bool) -> Result<(), AppError> {
    if raw_url.len() > 2048 {
        return Err(AppError::CallbackUrlInvalid);
    }

    let url = reqwest::Url::parse(raw_url).map_err(|_| AppError::CallbackUrlInvalid)?;

    if !allow_insecure && url.scheme() != "https" {
        return Err(AppError::CallbackUrlInvalid);
    }

    if allow_insecure && url.scheme() != "https" && url.scheme() != "http" {
        return Err(AppError::CallbackUrlInvalid);
    }

    let host = url.host_str().ok_or(AppError::CallbackUrlInvalid)?;
    if host.is_empty() {
        return Err(AppError::CallbackUrlInvalid);
    }

    // Tolak kredensial inline (https://user:pass@host) agar tidak bocor di log.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::CallbackUrlInvalid);
    }

    if allow_insecure {
        return Ok(());
    }

    let host_lower = host.to_ascii_lowercase();

    if let Ok(ip) = host_lower.parse::<IpAddr>() {
        if is_forbidden_ip(&ip) {
            return Err(AppError::CallbackUrlPrivate);
        }
        return Ok(());
    }

    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
    {
        return Err(AppError::CallbackUrlPrivate);
    }

    // Hostname tanpa titik tidak dapat dipublikkan secara DNS.
    if !host_lower.contains('.') {
        return Err(AppError::CallbackUrlPrivate);
    }

    Ok(())
}

fn is_forbidden_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                // 100.64.0.0/10 (CGNAT) & 169.254.0.0/16 sudah tercakup link-local
                || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]))
                // 0.0.0.0/8
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                // IPv4-mapped (::ffff:127.0.0.1)
                || v6.to_ipv4_mapped().is_some_and(|m| is_forbidden_ip(&IpAddr::V4(m)))
        }
    }
}

#[derive(sqlx::FromRow)]
struct DueDelivery {
    id: String,
    merchant_id: String,
    url: String,
    payload_json: String,
    attempts: i64,
}

#[derive(sqlx::FromRow)]
struct SigningSecretRow {
    id: String,
    secret_enc: String,
}

/// Worker outbox untuk mengirim callback merchant (RULES §12 OW-01..OW-12).
pub struct OutboundWorker {
    pool: DbPool,
    client: reqwest::Client,
    dek_b64: String,
    interval: Duration,
    batch_size: i64,
    allow_insecure: bool,
}

impl OutboundWorker {
    pub fn new(pool: DbPool, dek_b64: String, interval_seconds: u64, allow_insecure: bool) -> Self {
        // Redirect WAJIB dinonaktifkan (RL-15 / OW-05).
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent("qris-api/0.1")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            pool,
            client,
            dek_b64,
            interval: Duration::from_secs(interval_seconds),
            batch_size: 20,
            allow_insecure,
        }
    }

    pub async fn run(self) {
        info!(
            "Outbound worker dimulai: interval = {:?}, batch = {}",
            self.interval, self.batch_size
        );
        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;
            if let Err(e) = self.process_batch().await {
                error!("Outbound worker gagal memproses batch: {}", e);
            }
        }
    }

    /// Memproses semua delivery yang sudah jatuh tempo.
    pub async fn process_batch(&self) -> Result<usize, AppError> {
        let now = chrono::Utc::now().timestamp_millis();

        let due = sqlx::query_as::<_, DueDelivery>(
            r#"
            SELECT id, merchant_id, url, payload_json, attempts
            FROM webhook_deliveries
            WHERE status IN ('PENDING','FAILED')
              AND next_attempt_at <= ?
            ORDER BY next_attempt_at ASC
            LIMIT ?
            "#,
        )
        .bind(now)
        .bind(self.batch_size)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        let mut processed = 0usize;
        for delivery in due {
            self.attempt_delivery(delivery).await;
            processed += 1;
        }

        Ok(processed)
    }

    async fn attempt_delivery(&self, delivery: DueDelivery) {
        // 0. Validasi ulang URL saat kirim (mitigasi DNS rebinding, RL-14).
        if validate_callback_url(&delivery.url, self.allow_insecure).is_err() {
            self.mark_dead(
                &delivery.id,
                delivery.attempts,
                "callback_url_rejected",
                None,
            )
            .await;
            warn!(
                "Delivery {} ditolak: callback URL tidak lolos validasi SSRF",
                delivery.id
            );
            return;
        }

        // 1. Ambil signing secret ACTIVE milik merchant (OW-03).
        let secret_row = sqlx::query_as::<_, SigningSecretRow>(
            r#"
            SELECT id, secret_enc FROM signing_secrets
            WHERE merchant_id = ? AND status = 'ACTIVE'
            "#,
        )
        .bind(&delivery.merchant_id)
        .fetch_optional(&self.pool)
        .await;

        let secret_row = match secret_row {
            Ok(Some(r)) => r,
            Ok(None) => {
                // Tanpa signing secret, callback tidak dapat ditandatangani.
                self.mark_dead(
                    &delivery.id,
                    delivery.attempts,
                    "no_active_signing_secret",
                    None,
                )
                .await;
                warn!(
                    "Delivery {} tidak dapat dikirim: merchant tidak punya signing secret aktif",
                    delivery.id
                );
                return;
            }
            Err(e) => {
                error!(
                    "Gagal membaca signing secret untuk delivery {}: {}",
                    delivery.id, e
                );
                return;
            }
        };

        let aad = format!("signing_secrets:{}:{}", secret_row.id, delivery.merchant_id);
        let secret = match decrypt_secret(&secret_row.secret_enc, &self.dek_b64, &aad) {
            Ok(s) => s,
            Err(_) => {
                self.mark_dead(
                    &delivery.id,
                    delivery.attempts,
                    "secret_decrypt_failed",
                    None,
                )
                .await;
                error!(
                    "Delivery {} gagal: dekripsi signing secret tidak berhasil",
                    delivery.id
                );
                return;
            }
        };

        // 2. Susun signature: canonical = ts + "\n" + event_id + "\n" + sha256(body)
        //    Payload TIDAK dirakit ulang, memakai snapshot (OW-08).
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let mut hasher = Sha256::new();
        hasher.update(delivery.payload_json.as_bytes());
        let body_hash = format!("{:x}", hasher.finalize());

        let canonical = format!("{}\n{}\n{}", timestamp, delivery.id, body_hash);
        let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => return,
        };
        mac.update(canonical.as_bytes());
        let signature = format!("v1={:x}", mac.finalize().into_bytes());

        // 3. Kirim request. Tidak mengikuti redirect (OW-05).
        let response = self
            .client
            .post(&delivery.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("X-Qris-Timestamp", &timestamp)
            .header("X-Qris-Event-Id", &delivery.id)
            .header("X-Qris-Signature", &signature)
            .body(delivery.payload_json.clone())
            .send()
            .await;

        match response {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    self.mark_delivered(&delivery.id).await;
                    info!("Delivery {} terkirim (HTTP {})", delivery.id, status);
                } else if is_permanent_failure(status) {
                    // 4xx non-retryable → DEAD tanpa retry (OW-06).
                    self.mark_dead(
                        &delivery.id,
                        delivery.attempts,
                        &format!("http_{}", status),
                        Some(status as i64),
                    )
                    .await;
                    warn!(
                        "Delivery {} gagal permanen (HTTP {}), ditandai DEAD",
                        delivery.id, status
                    );
                } else {
                    self.schedule_retry(
                        &delivery,
                        &format!("http_{}", status),
                        Some(status as i64),
                    )
                    .await;
                }
            }
            Err(e) => {
                // Error jaringan/timeout. Hanya jenis error yang dicatat, bukan detail URL.
                let kind = if e.is_timeout() {
                    "timeout"
                } else if e.is_connect() {
                    "connect_failed"
                } else {
                    "transport_error"
                };
                self.schedule_retry(&delivery, kind, None).await;
            }
        }
    }

    async fn mark_delivered(&self, id: &str) {
        let now = chrono::Utc::now().timestamp_millis();
        if let Err(e) = sqlx::query(
            r#"
            UPDATE webhook_deliveries
            SET status = 'DELIVERED', delivered_at = ?, attempts = attempts + 1,
                last_error = NULL
            WHERE id = ?
            "#,
        )
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        {
            error!("Gagal menandai delivery {} sebagai DELIVERED: {}", id, e);
        }
    }

    async fn mark_dead(&self, id: &str, attempts: i64, reason: &str, status_code: Option<i64>) {
        if let Err(e) = sqlx::query(
            r#"
            UPDATE webhook_deliveries
            SET status = 'DEAD', attempts = ?, last_error = ?, last_status_code = ?
            WHERE id = ?
            "#,
        )
        .bind(attempts.max(1))
        .bind(reason)
        .bind(status_code)
        .bind(id)
        .execute(&self.pool)
        .await
        {
            error!("Gagal menandai delivery {} sebagai DEAD: {}", id, e);
        }
    }

    async fn schedule_retry(&self, delivery: &DueDelivery, reason: &str, status_code: Option<i64>) {
        let next_attempts = delivery.attempts + 1;

        if next_attempts >= MAX_ATTEMPTS {
            self.mark_dead(&delivery.id, next_attempts, reason, status_code)
                .await;
            warn!(
                "Delivery {} mencapai batas {} percobaan, ditandai DEAD",
                delivery.id, MAX_ATTEMPTS
            );
            return;
        }

        // BACKOFF_SECONDS[i] = jeda setelah percobaan ke-(i+1).
        let idx = (next_attempts as usize)
            .saturating_sub(1)
            .min(BACKOFF_SECONDS.len() - 1);
        let delay_ms = BACKOFF_SECONDS[idx] * 1000;
        let next_at = chrono::Utc::now().timestamp_millis() + delay_ms;

        if let Err(e) = sqlx::query(
            r#"
            UPDATE webhook_deliveries
            SET status = 'FAILED', attempts = ?, next_attempt_at = ?,
                last_error = ?, last_status_code = ?
            WHERE id = ?
            "#,
        )
        .bind(next_attempts)
        .bind(next_at)
        .bind(reason)
        .bind(status_code)
        .bind(&delivery.id)
        .execute(&self.pool)
        .await
        {
            error!("Gagal menjadwalkan retry delivery {}: {}", delivery.id, e);
        }
    }
}

/// 4xx yang tidak layak diretry (OW-06). 408 dan 429 tetap diretry.
fn is_permanent_failure(status: u16) -> bool {
    (400..500).contains(&status) && status != 408 && status != 429
}

/// Input pembangun payload snapshot callback `transaction.paid`.
pub struct PaidPayloadInput<'a> {
    pub event_id: &'a str,
    pub transaction_id: &'a str,
    pub merchant_id: &'a str,
    pub reference_label: Option<&'a str>,
    pub base: i64,
    pub fee: i64,
    pub unique_code: i64,
    pub final_amount: i64,
    pub paid_at_ms: i64,
    pub payment_source: Option<&'a str>,
}

/// Membangun payload snapshot callback `transaction.paid` (ERP §9.x, OW-10).
pub fn build_paid_payload(input: PaidPayloadInput<'_>) -> Value {
    let created_at = rfc3339_from_ms(input.paid_at_ms);
    serde_json::json!({
        "event": "transaction.paid",
        "event_id": input.event_id,
        "created_at": created_at,
        "data": {
            "transaction_id": input.transaction_id,
            "merchant_id": input.merchant_id,
            "reference_label": input.reference_label,
            "amount": {
                "base": input.base,
                "fee": input.fee,
                "unique_code": input.unique_code,
                "final": input.final_amount,
                "currency": "IDR"
            },
            "status": "PAID",
            "paid_at": created_at,
            "payment_source": input.payment_source
        }
    })
}

fn rfc3339_from_ms(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_non_https_callback() {
        assert!(matches!(
            validate_callback_url("http://app.example.com/hook", false),
            Err(AppError::CallbackUrlInvalid)
        ));
    }

    #[test]
    fn test_insecure_mode_allows_local_for_dev() {
        assert!(validate_callback_url("http://127.0.0.1:8080/hook", true).is_ok());
        assert!(validate_callback_url("http://127.0.0.1:8080/hook", false).is_err());
    }

    #[test]
    fn test_reject_loopback_and_private_literal() {
        for url in [
            "https://127.0.0.1/hook",
            "https://localhost/hook",
            "https://10.0.0.5/hook",
            "https://192.168.1.1/hook",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/hook",
            "https://0.0.0.0/hook",
        ] {
            assert!(
                matches!(
                    validate_callback_url(url, false),
                    Err(AppError::CallbackUrlPrivate)
                ),
                "{url} seharusnya ditolak sebagai jaringan privat"
            );
        }
    }

    #[test]
    fn test_reject_internal_hostnames() {
        for url in [
            "https://api.internal/hook",
            "https://printer.local/hook",
            "https://intranet/hook",
        ] {
            assert!(matches!(
                validate_callback_url(url, false),
                Err(AppError::CallbackUrlPrivate)
            ));
        }
    }

    #[test]
    fn test_accept_public_https() {
        assert!(validate_callback_url("https://app.example.com/webhooks/qris", false).is_ok());
        assert!(validate_callback_url("https://8.8.8.8/hook", false).is_ok());
    }

    #[test]
    fn test_permanent_failure_classification() {
        assert!(is_permanent_failure(400));
        assert!(is_permanent_failure(404));
        assert!(is_permanent_failure(422));
        assert!(!is_permanent_failure(408));
        assert!(!is_permanent_failure(429));
        assert!(!is_permanent_failure(500));
        assert!(!is_permanent_failure(302));
    }

    #[test]
    fn test_paid_payload_has_no_secret() {
        let p = build_paid_payload(PaidPayloadInput {
            event_id: "dlv_1",
            transaction_id: "txn_1",
            merchant_id: "mch_1",
            reference_label: Some("INV-1"),
            base: 500_000,
            fee: 2_500,
            unique_code: 142,
            final_amount: 502_642,
            paid_at_ms: 1_767_225_600_000,
            payment_source: Some("ANDROID"),
        });
        let s = p.to_string();
        assert!(!s.contains("secret"));
        assert!(!s.contains("api_key"));
        assert_eq!(p["data"]["amount"]["final"], 502_642);
        assert_eq!(p["event_id"], "dlv_1");
    }
}
