use crate::db::DbPool;
use crate::domain::amount::{FeeConfig, FeeMode, Rupiah, calculate_final_amount};
use crate::domain::qris::transform_to_dynamic;
use crate::error::AppError;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use ulid::Ulid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Transaction {
    pub id: String,
    pub merchant_id: String,
    pub merchant_ref: Option<String>,
    pub reference_label: Option<String>,
    pub idempotency_key: Option<String>,
    pub base_amount: i64,
    pub fee_mode: String,
    pub fee_fixed: i64,
    pub fee_percent_bps: i64,
    pub fee_amount: i64,
    pub unique_code: i64,
    pub final_amount: i64,
    pub currency: String,
    pub dynamic_payload: String,
    pub dynamic_payload_sha256: String,
    pub crc: String,
    pub status: String,
    pub payment_source: Option<String>,
    pub paid_event_id: Option<String>,
    pub expires_at: i64,
    pub paid_at: Option<i64>,
    pub canceled_at: Option<i64>,
    pub cancel_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateTransactionParams {
    pub base_amount: i64,
    pub merchant_ref: Option<String>,
    pub reference_label: Option<String>,
    pub idempotency_key: Option<String>,
    pub fee: Option<FeeConfig>,
    pub unique_code_enabled: Option<bool>,
    pub unique_code_value: Option<i32>,
    pub ttl_seconds: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct MerchantConfigRow {
    status: String,
    static_payload: String,
    default_fee_mode: String,
    default_fee_fixed: i64,
    default_fee_percent_bps: i64,
    default_unique_code: i64,
    default_ttl_seconds: i64,
}

pub struct TransactionService<'a> {
    pool: &'a DbPool,
}

impl<'a> TransactionService<'a> {
    pub fn new(pool: &'a DbPool) -> Self {
        Self { pool }
    }

    pub async fn create_transaction(
        &self,
        merchant_id: &str,
        params: CreateTransactionParams,
    ) -> Result<Transaction, AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        // 1. Ambil profil merchant
        let m = sqlx::query_as::<_, MerchantConfigRow>(
            r#"
            SELECT id, status, static_payload, default_fee_mode,
                   default_fee_fixed, default_fee_percent_bps, default_unique_code,
                   default_ttl_seconds
            FROM merchants
            WHERE id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(merchant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or(AppError::MerchantNotFound)?;

        if m.status != "ACTIVE" {
            return Err(AppError::MerchantInactive);
        }

        // 2. Cek Idempotency Key (FR-TXN-06)
        if let Some(ref idem) = params.idempotency_key {
            let existing = sqlx::query_as::<_, Transaction>(
                "SELECT * FROM transactions WHERE merchant_id = ? AND idempotency_key = ?",
            )
            .bind(merchant_id)
            .bind(idem)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

            if let Some(ext) = existing {
                // Periksa apakah request body sama persis
                if ext.base_amount == params.base_amount {
                    tx.commit()
                        .await
                        .map_err(|e| AppError::Internal(e.to_string()))?;
                    return Ok(ext);
                } else {
                    return Err(AppError::TxnIdempotencyConflict);
                }
            }
        }

        // 3. Konfigurasi fee & TTL
        let fee_cfg = params.fee.unwrap_or_else(|| {
            let mode = match m.default_fee_mode.as_str() {
                "FIXED" => FeeMode::Fixed,
                "PERCENT" => FeeMode::Percent,
                _ => FeeMode::None,
            };
            FeeConfig {
                mode,
                fixed: m.default_fee_fixed,
                percent_bps: m.default_fee_percent_bps as i32,
            }
        });

        let ttl = params.ttl_seconds.unwrap_or(m.default_ttl_seconds);
        if !(60..=86400).contains(&ttl) {
            return Err(AppError::TxnTtlInvalid);
        }

        let unique_enabled = params
            .unique_code_enabled
            .unwrap_or(m.default_unique_code == 1);

        // 4. Alokasi kode unik (ERP §6.3)
        let chosen_code = if unique_enabled {
            if let Some(manual_c) = params.unique_code_value {
                if !(1..=999).contains(&manual_c) {
                    return Err(AppError::QrisAmountInvalid);
                }
                let check_manual = sqlx::query_scalar::<_, i64>(
                    "SELECT 1 FROM transactions WHERE merchant_id = ? AND status = 'PENDING' AND unique_code = ?",
                )
                .bind(merchant_id)
                .bind(manual_c)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

                if check_manual.is_some() {
                    return Err(AppError::UniqueCodeUnavailable);
                }
                manual_c
            } else {
                // Generate acak 1..=999
                let mut allocated = None;
                for _ in 0..999 {
                    let candidate = rand::thread_rng().gen_range(1..=999);
                    let check = sqlx::query_scalar::<_, i64>(
                        "SELECT 1 FROM transactions WHERE merchant_id = ? AND status = 'PENDING' AND unique_code = ?",
                    )
                    .bind(merchant_id)
                    .bind(candidate)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| AppError::Internal(e.to_string()))?;

                    if check.is_none() {
                        allocated = Some(candidate);
                        break;
                    }
                }
                allocated.ok_or(AppError::UniqueCodePoolExhausted)?
            }
        } else {
            0
        };

        // 5. Kalkulasi final amount
        let breakdown = calculate_final_amount(
            Rupiah::new(params.base_amount)?,
            &fee_cfg,
            if unique_enabled {
                Some(chosen_code)
            } else {
                None
            },
        )?;

        // Cek tabrakan final_amount dengan transaksi PENDING lain (RULES §4 AM-17)
        let conflict_amount = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM transactions WHERE merchant_id = ? AND status = 'PENDING' AND final_amount = ?",
        )
        .bind(merchant_id)
        .bind(breakdown.final_amount)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if conflict_amount.is_some() {
            return Err(AppError::TxnAmountCollision);
        }

        // 6. Transformasi QRIS statis -> dinamis
        let dynamic_payload = transform_to_dynamic(&m.static_payload, breakdown.final_amount)?;
        let crc = dynamic_payload[dynamic_payload.len() - 4..].to_string();

        let mut hasher = Sha256::new();
        hasher.update(dynamic_payload.as_bytes());
        let payload_sha256 = format!("{:x}", hasher.finalize());

        let txn_id = format!("txn_{}", Ulid::new());
        let now = chrono::Utc::now().timestamp_millis();
        let expires_at = now + (ttl * 1000);

        let fee_mode_str = match fee_cfg.mode {
            FeeMode::Fixed => "FIXED",
            FeeMode::Percent => "PERCENT",
            FeeMode::None => "NONE",
        };
        let unique_code_i64 = chosen_code as i64;
        let fee_bps_i64 = fee_cfg.percent_bps as i64;

        // 7. Insert transaksi
        sqlx::query(
            r#"
            INSERT INTO transactions (
                id, merchant_id, merchant_ref, reference_label, idempotency_key,
                base_amount, fee_mode, fee_fixed, fee_percent_bps, fee_amount,
                unique_code, final_amount, currency, dynamic_payload, dynamic_payload_sha256,
                crc, status, expires_at, created_at, updated_at
            ) VALUES (
                ?, ?, ?, ?, ?,
                ?, ?, ?, ?, ?,
                ?, ?, 'IDR', ?, ?,
                ?, 'PENDING', ?, ?, ?
            )
            "#,
        )
        .bind(&txn_id)
        .bind(merchant_id)
        .bind(&params.merchant_ref)
        .bind(&params.reference_label)
        .bind(&params.idempotency_key)
        .bind(breakdown.base)
        .bind(fee_mode_str)
        .bind(fee_cfg.fixed)
        .bind(fee_bps_i64)
        .bind(breakdown.fee)
        .bind(unique_code_i64)
        .bind(breakdown.final_amount)
        .bind(&dynamic_payload)
        .bind(&payload_sha256)
        .bind(&crc)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        self.get_transaction(&txn_id).await
    }

    pub async fn get_transaction(&self, id: &str) -> Result<Transaction, AppError> {
        sqlx::query_as::<_, Transaction>(
            r#"
            SELECT
                id, merchant_id, merchant_ref, reference_label, idempotency_key,
                base_amount, fee_mode, fee_fixed, fee_percent_bps, fee_amount,
                unique_code, final_amount, currency, dynamic_payload, dynamic_payload_sha256,
                crc, status, payment_source, paid_event_id, expires_at, paid_at,
                canceled_at, cancel_reason, created_at, updated_at
            FROM transactions
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or(AppError::TxnNotFound)
    }

    pub async fn cancel_transaction(
        &self,
        id: &str,
        merchant_id: &str,
        reason: Option<String>,
    ) -> Result<Transaction, AppError> {
        let txn = self.get_transaction(id).await?;
        if txn.merchant_id != merchant_id {
            return Err(AppError::AuthForbidden);
        }
        if txn.status != "PENDING" {
            return Err(AppError::TxnNotPending);
        }

        let now = chrono::Utc::now().timestamp_millis();
        let rows = sqlx::query(
            r#"
            UPDATE transactions
            SET status = 'CANCELED', canceled_at = ?, cancel_reason = ?, updated_at = ?
            WHERE id = ? AND status = 'PENDING'
            "#,
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .rows_affected();

        if rows == 0 {
            return Err(AppError::TxnNotPending);
        }

        self.get_transaction(id).await
    }

    /// Menandai transaksi lunas secara manual oleh admin master (TX-06, TX-07, MT-12).
    /// Boleh dari PENDING, EXPIRED, CANCELED, AMBIGUOUS.
    /// Dilarang dari PAID (409 TXN_ALREADY_PAID).
    /// Alasan wajib 3–200 karakter.
    pub async fn mark_paid(&self, id: &str, reason: &str) -> Result<Transaction, AppError> {
        let reason_trimmed = reason.trim();
        let len = reason_trimmed.chars().count();
        if !(3..=200).contains(&len) {
            return Err(AppError::ValidationError(
                "Alasan mark-paid wajib berukuran 3 hingga 200 karakter.".to_string(),
            ));
        }

        let txn = self.get_transaction(id).await?;
        if txn.status == "PAID" {
            return Err(AppError::TxnAlreadyPaid);
        }

        let now = chrono::Utc::now().timestamp_millis();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let rows = sqlx::query(
            r#"
            UPDATE transactions
            SET status = 'PAID',
                paid_at = ?,
                payment_source = 'MANUAL',
                updated_at = ?
            WHERE id = ? AND status != 'PAID'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .rows_affected();

        if rows == 0 {
            return Err(AppError::TxnAlreadyPaid);
        }

        // Simpan outbound webhook event jika merchant memiliki callback_url
        let merchant_cb: Option<String> =
            sqlx::query_scalar("SELECT callback_url FROM merchants WHERE id = ?")
                .bind(&txn.merchant_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?
                .flatten();

        if let Some(cb_url) = merchant_cb {
            let delivery_id = format!("dlv_{}", ulid::Ulid::new());
            let payload = serde_json::json!({
                "event": "transaction.paid",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "data": {
                    "id": txn.id,
                    "merchant_id": txn.merchant_id,
                    "merchant_ref": txn.merchant_ref,
                    "amount": txn.final_amount,
                    "status": "PAID",
                    "payment_source": "MANUAL",
                    "paid_at": chrono::Utc::now().to_rfc3339(),
                    "reason": reason_trimmed,
                }
            });

            let _ = sqlx::query(
                r#"
                INSERT INTO webhook_deliveries (
                    id, merchant_id, event_type, url, payload_json,
                    status, attempts, next_attempt_at, created_at
                )
                VALUES (?, ?, 'transaction.paid', ?, ?, 'PENDING', 0, ?, ?)
                "#,
            )
            .bind(delivery_id)
            .bind(&txn.merchant_id)
            .bind(cb_url)
            .bind(payload.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await;
        }

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        self.get_transaction(id).await
    }
}
