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
    /// Validasi HMAC signature + anti-replay nonce untuk request non-GET
    /// (RULES §8 SG-01 s/d SG-16).
    pub async fn validate(
        state: &AppState,
        principal: &Principal,
        params: RequestSignatureParams<'_>,
    ) -> Result<(), AppError> {
        // Mode strict non-aktif (dev) → lewati.
        if !state.config.require_signed_requests {
            return Ok(());
        }

        let ts_str = params.timestamp_str.ok_or(AppError::SignatureRequired)?;
        let nonce = params.nonce_str.ok_or(AppError::SignatureRequired)?;
        let signature = params.signature_str.ok_or(AppError::SignatureRequired)?;

        // SG-03: nonce 16-64 karakter ASCII.
        if !(16..=64).contains(&nonce.len()) || !nonce.is_ascii() {
            return Err(AppError::SignatureInvalid);
        }

        // SG-02: timestamp epoch detik, skew maksimal 300 detik.
        let req_ts: i64 = ts_str
            .parse()
            .map_err(|_| AppError::SignatureTimestampInvalid)?;
        let now_sec = chrono::Utc::now().timestamp();
        let skew_limit = 300i64;

        if (now_sec - req_ts).abs() > skew_limit {
            return Err(AppError::SignatureTimestampInvalid);
        }

        // SG-08: master memakai MASTER_API_KEY, merchant memakai signing secret ACTIVE.
        let (credential_id, secret) = match principal {
            Principal::Master => ("master".to_string(), state.config.master_api_key.clone()),
            Principal::Merchant { merchant_id, .. } => {
                let row = sqlx::query_as::<_, (String, String)>(
                    r#"
                    SELECT id, secret_enc FROM signing_secrets
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

                // AAD WAJIB sama persis dengan saat enkripsi (merchant_service).
                let aad = format!("signing_secrets:{}:{}", row.0, merchant_id);
                let sec = decrypt_secret(&row.1, &state.config.data_encryption_key, &aad)
                    .map_err(|_| AppError::Internal("Gagal dekripsi signing secret".to_string()))?;

                (merchant_id.clone(), sec)
            }
        };

        // SG-04/SG-05/SG-06: kanonik memakai path+query apa adanya.
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

        // SG-09: perbandingan constant-time.
        let is_match: bool = expected_sig.as_bytes().ct_eq(signature.as_bytes()).into();
        if !is_match {
            return Err(AppError::SignatureInvalid);
        }

        // SG-10/SG-16: nonce disimpan setelah signature terbukti valid, sehingga
        // request dengan signature salah tidak membakar nonce milik client sah.
        // Unique constraint (credential_id, nonce) yang menjamin anti-replay.
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

        match insert_res {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                Err(AppError::SignatureReplayed)
            }
            Err(e) => Err(AppError::Internal(e.to_string())),
        }
    }
}
