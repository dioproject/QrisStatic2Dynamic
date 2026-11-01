use crate::auth::crypto::{encrypt_secret, generate_token, hash_api_key};
use crate::db::DbPool;
use crate::domain::qris::parse_qris;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use ulid::Ulid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Merchant {
    pub id: String,
    pub name: String,
    pub status: String,
    pub static_payload: String,
    pub static_payload_sha256: String,
    pub merchant_name: String,
    pub merchant_city: String,
    pub postal_code: Option<String>,
    pub mcc: String,
    pub nmid: Option<String>,
    pub acquirer_guid: Option<String>,
    pub merchant_criteria: Option<String>,
    pub default_fee_mode: String,
    pub default_fee_fixed: i64,
    pub default_fee_percent_bps: i64,
    pub default_unique_code: i64,
    pub default_ttl_seconds: i64,
    pub callback_url: Option<String>,
    pub callback_events: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreatedCredentials {
    pub api_key: String,
    pub key_prefix: String,
    pub signing_secret: String,
    pub secret_prefix: String,
    pub note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CreateMerchantResult {
    pub merchant: Merchant,
    pub credentials: CreatedCredentials,
}

pub struct MerchantService<'a> {
    pool: &'a DbPool,
    dek_b64: &'a str,
}

impl<'a> MerchantService<'a> {
    pub fn new(pool: &'a DbPool, dek_b64: &'a str) -> Self {
        Self { pool, dek_b64 }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_merchant(
        &self,
        name: &str,
        static_payload: &str,
        default_fee_mode: &str,
        default_fee_fixed: i64,
        default_fee_percent_bps: i32,
        default_unique_code: bool,
        default_ttl_seconds: i64,
        callback_url: Option<&str>,
    ) -> Result<CreateMerchantResult, AppError> {
        let parsed = parse_qris(static_payload, false)?;
        let now = chrono::Utc::now().timestamp_millis();
        let merchant_id = format!("mch_{}", Ulid::new());

        // Mulai transaksi database
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        // Cek konflik payload atau NMID (FR-MCH-06)
        if let Some(ref nmid) = parsed.merchant.nmid {
            let conflict_nmid = sqlx::query_scalar::<_, String>(
                "SELECT id FROM merchants WHERE nmid = ? AND deleted_at IS NULL",
            )
            .bind(nmid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

            if conflict_nmid.is_some() {
                return Err(AppError::MerchantNmidConflict);
            }
        }

        let conflict_payload = sqlx::query_scalar::<_, String>(
            "SELECT id FROM merchants WHERE static_payload_sha256 = ? AND deleted_at IS NULL",
        )
        .bind(&parsed.payload_sha256)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if conflict_payload.is_some() {
            return Err(AppError::MerchantAlreadyRegistered);
        }

        let default_unique_int = if default_unique_code { 1i64 } else { 0i64 };
        let default_bps_i64 = default_fee_percent_bps as i64;

        // 1. Simpan data merchant
        sqlx::query(
            r#"
            INSERT INTO merchants (
                id, name, status, static_payload, static_payload_sha256,
                merchant_name, merchant_city, postal_code, mcc, nmid,
                acquirer_guid, merchant_criteria, default_fee_mode,
                default_fee_fixed, default_fee_percent_bps, default_unique_code,
                default_ttl_seconds, callback_url, callback_events,
                created_at, updated_at
            ) VALUES (
                ?, ?, 'ACTIVE', ?, ?,
                ?, ?, ?, ?, ?,
                ?, ?, ?,
                ?, ?, ?,
                ?, ?, '["transaction.paid"]',
                ?, ?
            )
            "#,
        )
        .bind(&merchant_id)
        .bind(name)
        .bind(&parsed.payload)
        .bind(&parsed.payload_sha256)
        .bind(&parsed.merchant.name)
        .bind(&parsed.merchant.city)
        .bind(&parsed.merchant.postal_code)
        .bind(&parsed.merchant.mcc)
        .bind(&parsed.merchant.nmid)
        .bind(&parsed.merchant.acquirer_guid)
        .bind(&parsed.merchant.merchant_criteria)
        .bind(default_fee_mode)
        .bind(default_fee_fixed)
        .bind(default_bps_i64)
        .bind(default_unique_int)
        .bind(default_ttl_seconds)
        .bind(callback_url)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        // 2. Buat API Key (qsk_live_...)
        let key_id = format!("key_{}", Ulid::new());
        let api_key = generate_token("qsk_live_", 40);
        let key_prefix = api_key[..16].to_string();
        let key_hash = hash_api_key(&api_key);

        sqlx::query(
            r#"
            INSERT INTO merchant_keys (id, merchant_id, key_prefix, key_hash, label, status, created_at)
            VALUES (?, ?, ?, ?, 'default', 'ACTIVE', ?)
            "#,
        )
        .bind(&key_id)
        .bind(&merchant_id)
        .bind(&key_prefix)
        .bind(&key_hash)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        // 3. Buat Signing Secret (qss_live_...)
        let secret_id = format!("sec_{}", Ulid::new());
        let signing_secret = generate_token("qss_live_", 40);
        let secret_prefix = signing_secret[..16].to_string();
        let aad = format!("signing_secrets:{}:{}", secret_id, merchant_id);
        let secret_enc = encrypt_secret(&signing_secret, self.dek_b64, &aad)?;

        sqlx::query(
            r#"
            INSERT INTO signing_secrets (id, merchant_id, secret_prefix, secret_enc, status, created_at)
            VALUES (?, ?, ?, ?, 'ACTIVE', ?)
            "#,
        )
        .bind(&secret_id)
        .bind(&merchant_id)
        .bind(&secret_prefix)
        .bind(&secret_enc)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let merchant = self.get_merchant(&merchant_id).await?;

        Ok(CreateMerchantResult {
            merchant,
            credentials: CreatedCredentials {
                api_key,
                key_prefix,
                signing_secret,
                secret_prefix,
                note: "Simpan sekarang. Key dan secret tidak akan ditampilkan lagi.",
            },
        })
    }

    pub async fn get_merchant(&self, merchant_id: &str) -> Result<Merchant, AppError> {
        sqlx::query_as::<_, Merchant>(
            r#"
            SELECT
                id, name, status, static_payload, static_payload_sha256,
                merchant_name, merchant_city, postal_code, mcc, nmid,
                acquirer_guid, merchant_criteria, default_fee_mode,
                default_fee_fixed, default_fee_percent_bps, default_unique_code,
                default_ttl_seconds, callback_url, callback_events,
                created_at, updated_at, deleted_at
            FROM merchants
            WHERE id = ? AND deleted_at IS NULL
            "#,
        )
        .bind(merchant_id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or(AppError::MerchantNotFound)
    }

    pub async fn list_merchants(&self, limit: i64) -> Result<Vec<Merchant>, AppError> {
        let lim = limit.clamp(1, 100);
        sqlx::query_as::<_, Merchant>(
            r#"
            SELECT
                id, name, status, static_payload, static_payload_sha256,
                merchant_name, merchant_city, postal_code, mcc, nmid,
                acquirer_guid, merchant_criteria, default_fee_mode,
                default_fee_fixed, default_fee_percent_bps, default_unique_code,
                default_ttl_seconds, callback_url, callback_events,
                created_at, updated_at, deleted_at
            FROM merchants
            WHERE deleted_at IS NULL
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(lim)
        .fetch_all(self.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
    }
}
