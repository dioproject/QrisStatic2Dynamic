pub mod crypto;
pub mod hmac;

use crate::auth::crypto::verify_master_key;
use crate::error::AppError;
use crate::state::AppState;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Debug, Clone)]
pub enum Principal {
    Master,
    Merchant { merchant_id: String, key_id: String },
}

impl Principal {
    pub fn is_master(&self) -> bool {
        matches!(self, Self::Master)
    }

    pub fn merchant_id(&self) -> Option<&str> {
        match self {
            Self::Master => None,
            Self::Merchant { merchant_id, .. } => Some(merchant_id.as_str()),
        }
    }
}

pub struct RequireAuth(pub Principal);
pub struct RequireMaster(pub ());

#[derive(sqlx::FromRow)]
struct KeyLookup {
    key_id: String,
    key_hash: String,
    key_status: String,
    merchant_id: String,
    merchant_status: String,
}

impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state_ref: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state_ref);
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::AuthMissing)?;

        if !auth_header.starts_with("Bearer ") {
            return Err(AppError::AuthInvalid);
        }

        let token = auth_header[7..].trim();
        let principal = resolve_principal(&state, token).await?;
        Ok(RequireAuth(principal))
    }
}

/// Resolusi token Bearer menjadi `Principal`.
///
/// Dipakai oleh extractor `RequireAuth` dan oleh middleware signed request,
/// sehingga aturan pengecekan master/merchant hanya ada di satu tempat.
pub async fn resolve_principal(state: &AppState, token: &str) -> Result<Principal, AppError> {
    if token.is_empty() {
        return Err(AppError::AuthInvalid);
    }

    // 1. Cek Master Key (constant-time)
    if verify_master_key(
        token,
        &state.config.master_api_key,
        state.config.master_api_key_previous.as_deref(),
    ) {
        return Ok(Principal::Master);
    }

    // 2. Cek Merchant Key (Prefix Lookup + Constant-time hash check)
    if token.starts_with("qsk_live_") && token.len() >= 16 {
        let prefix = &token[..16];

        let row = sqlx::query_as::<_, KeyLookup>(
            r#"
            SELECT mk.id as key_id, mk.key_hash, mk.status as key_status,
                   m.id as merchant_id, m.status as merchant_status
            FROM merchant_keys mk
            JOIN merchants m ON m.id = mk.merchant_id
            WHERE mk.key_prefix = ?
            "#,
        )
        .bind(prefix)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if let Some(r) = row {
            let mut hasher = Sha256::new();
            hasher.update(token.as_bytes());
            let computed_hash = format!("{:x}", hasher.finalize());

            if computed_hash.as_bytes().ct_eq(r.key_hash.as_bytes()).into() {
                if r.key_status != "ACTIVE" {
                    return Err(AppError::AuthKeyRevoked);
                }
                if r.merchant_status != "ACTIVE" {
                    return Err(AppError::MerchantInactive);
                }

                return Ok(Principal::Merchant {
                    merchant_id: r.merchant_id,
                    key_id: r.key_id,
                });
            }
        }
    }

    Err(AppError::AuthInvalid)
}

impl<S> FromRequestParts<S> for RequireMaster
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state_ref: &S) -> Result<Self, Self::Rejection> {
        let RequireAuth(principal) = RequireAuth::from_request_parts(parts, state_ref).await?;
        if principal.is_master() {
            Ok(RequireMaster(()))
        } else {
            Err(AppError::AuthForbidden)
        }
    }
}
