use crate::auth::Principal;
use crate::auth::crypto::decrypt_secret;
use crate::error::AppError;
use crate::state::AppState;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub struct RequestSignatureParams<'a> {
    pub method: &'a str,
    pub path_and_query: &'a str,
    pub raw_body: &'a [u8],
    pub timestamp_str: Option<&'a str>,
    pub nonce_str: Option<&'a str>,
    pub signature_str: Option<&'a str>,
}

pub struct SignedRequestValidator;

impl SignedRequestValidator {
    /// Validasi HMAC signature + anti-replay nonce untuk request non-GET (RULES §8 SG-01 s/d SG-16)
    pub async fn validate(
        state: &AppState,
        principal: &Principal,
        params: RequestSignatureParams<'_>,
    ) -> Result<(), AppError> {
        // Jika mode strict tanda tangan non-aktif (hanya untuk dev), lewati
        if !state.config.require_signed_requests {
            return Ok(());
        }

        let ts_str = params.timestamp_str.ok_or(AppError::SignatureRequired)?;
        let nonce = params.nonce_str.ok_or(AppError::SignatureRequired)?;
        let signature = params.signature_str.ok_or(AppError::SignatureRequired)?;

        if !(16..=64).contains(&nonce.len()) {
            return Err(AppError::SignatureInvalid);
        }

        let req_ts: i64 = ts_str
            .parse()
            .map_err(|_| AppError::SignatureTimestampInvalid)?;
        let now_sec = chrono::Utc::now().timestamp();
        let skew_limit = 300i64; // 300 detik

        if (now_sec - req_ts).abs() > skew_limit {
            return Err(AppError::SignatureTimestampInvalid);
        }

        // 1. Dapatkan credential_id dan signing secret
        let (credential_id, secret) = match principal {
            Principal::Master => ("master".to_string(), state.config.master_api_key.clone()),
            Principal::Merchant {
                merchant_id,
                key_id,
            } => {
                let row = sqlx::query_scalar::<_, String>(
                    r#"
                    SELECT secret_enc FROM signing_secrets
                    WHERE merchant_id = ? AND status = 'ACTIVE'
                    "#,
                )
                .bind(merchant_id)
                .fetch_optional(&state.db)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?
                .ok_or_else(|| {
                    AppError::Internal("Signing secret aktif tidak ditemukan".to_string())
                })?;

                let aad = format!("signing_secrets:{}:{}", key_id, merchant_id);
                // Fallback AAD jika secret_id berbeda
                let sec = decrypt_secret(&row, &state.config.data_encryption_key, &aad)
                    .or_else(|_| decrypt_secret(&row, &state.config.data_encryption_key, ""))
                    .map_err(|_| AppError::Internal("Gagal dekripsi signing secret".to_string()))?;

                (merchant_id.clone(), sec)
            }
        };

        // 2. Anti-Replay: Cek dan simpan nonce di DB (SG-10)
        let now_ms = chrono::Utc::now().timestamp_millis();
        let insert_res = sqlx::query(
            r#"
            INSERT INTO request_nonces (credential_id, nonce, seen_at)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(&credential_id)
        .bind(nonce)
        .bind(now_ms)
        .execute(&state.db)
        .await;

        if insert_res.is_err() {
            return Err(AppError::SignatureReplayed);
        }

        // 3. Hitung Kanonik
        // canonical = timestamp + "\n" + nonce + "\n" + METHOD + "\n" + path_and_query + "\n" + lower_hex(sha256(raw_body))
        let mut hasher = Sha256::new();
        hasher.update(params.raw_body);
        let body_hash_hex = format!("{:x}", hasher.finalize());

        let canonical = format!(
            "{}\n{}\n{}\n{}\n{}",
            ts_str, nonce, params.method, params.path_and_query, body_hash_hex
        );

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| AppError::Internal(e.to_string()))?;
        mac.update(canonical.as_bytes());
        let expected_sig = format!("v1={:x}", mac.finalize().into_bytes());

        let is_match: bool = expected_sig.as_bytes().ct_eq(signature.as_bytes()).into();
        if !is_match {
            return Err(AppError::SignatureInvalid);
        }

        Ok(())
    }
}
