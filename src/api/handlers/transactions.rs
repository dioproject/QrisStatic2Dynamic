use crate::auth::{Principal, RequireAuth};
use crate::domain::amount::FeeConfig;
use crate::domain::render::{EccLevel, render_qr_png, render_qr_svg};
use crate::error::AppError;
use crate::services::transaction_service::{CreateTransactionParams, TransactionService};
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CreateTransactionPayload {
    pub base_amount: i64,
    pub merchant_ref: Option<String>,
    pub reference_label: Option<String>,
    pub fee: Option<FeeConfig>,
    pub unique_code: Option<crate::domain::amount::UniqueCodeConfig>,
    pub ttl_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TransactionStatusResponse {
    pub id: String,
    pub status: String,
    pub base_amount: i64,
    pub fee_amount: i64,
    pub unique_code: i64,
    pub final_amount: i64,
    pub expires_at: i64,
    pub paid_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CancelTransactionPayload {
    pub reason: Option<String>,
}

/// POST /api/v1/merchants/{id}/transactions
pub async fn create_transaction_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(merchant_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<CreateTransactionPayload>,
) -> Result<impl IntoResponse, AppError> {
    if let Principal::Merchant {
        merchant_id: m_id, ..
    } = &principal
        && m_id != &merchant_id
    {
        return Err(AppError::AuthForbidden);
    }

    let idem_header = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let (u_enabled, u_val) = if let Some(u) = payload.unique_code {
        (Some(u.enabled), u.value)
    } else {
        (None, None)
    };

    let params = CreateTransactionParams {
        base_amount: payload.base_amount,
        merchant_ref: payload.merchant_ref,
        reference_label: payload.reference_label,
        idempotency_key: idem_header,
        fee: payload.fee,
        unique_code_enabled: u_enabled,
        unique_code_value: u_val,
        ttl_seconds: payload.ttl_seconds,
    };

    let service = TransactionService::new(&state.db);
    let txn = service.create_transaction(&merchant_id, params).await?;

    Ok((StatusCode::CREATED, Json(txn)))
}

/// GET /api/v1/transactions/{id}
pub async fn get_transaction_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let service = TransactionService::new(&state.db);
    let txn = service.get_transaction(&id).await?;

    if let Principal::Merchant { merchant_id, .. } = &principal
        && merchant_id != &txn.merchant_id
    {
        return Err(AppError::AuthForbidden);
    }

    Ok(Json(txn))
}

/// GET /api/v1/transactions/{id}/status
pub async fn get_transaction_status_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let service = TransactionService::new(&state.db);
    let txn = service.get_transaction(&id).await?;

    if let Principal::Merchant { merchant_id, .. } = &principal
        && merchant_id != &txn.merchant_id
    {
        return Err(AppError::AuthForbidden);
    }

    Ok(Json(TransactionStatusResponse {
        id: txn.id,
        status: txn.status,
        base_amount: txn.base_amount,
        fee_amount: txn.fee_amount,
        unique_code: txn.unique_code,
        final_amount: txn.final_amount,
        expires_at: txn.expires_at,
        paid_at: txn.paid_at,
    }))
}

/// GET /api/v1/transactions/{id}/qr.png
pub async fn get_transaction_png_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let service = TransactionService::new(&state.db);
    let txn = service.get_transaction(&id).await?;

    if let Principal::Merchant { merchant_id, .. } = &principal
        && merchant_id != &txn.merchant_id
    {
        return Err(AppError::AuthForbidden);
    }

    let png_bytes =
        tokio::task::spawn_blocking(move || render_qr_png(&txn.dynamic_payload, 8, EccLevel::M))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        png_bytes,
    ))
}

/// GET /api/v1/transactions/{id}/qr.svg
pub async fn get_transaction_svg_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let service = TransactionService::new(&state.db);
    let txn = service.get_transaction(&id).await?;

    if let Principal::Merchant { merchant_id, .. } = &principal
        && merchant_id != &txn.merchant_id
    {
        return Err(AppError::AuthForbidden);
    }

    let svg_str =
        tokio::task::spawn_blocking(move || render_qr_svg(&txn.dynamic_payload, EccLevel::M))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok((
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        svg_str,
    ))
}

/// POST /api/v1/transactions/{id}/cancel
pub async fn cancel_transaction_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(id): Path<String>,
    Json(payload): Json<CancelTransactionPayload>,
) -> Result<impl IntoResponse, AppError> {
    let service = TransactionService::new(&state.db);
    let txn = service.get_transaction(&id).await?;

    if let Principal::Merchant { merchant_id, .. } = &principal
        && merchant_id != &txn.merchant_id
    {
        return Err(AppError::AuthForbidden);
    }

    let updated = service
        .cancel_transaction(&id, &txn.merchant_id, payload.reason)
        .await?;

    Ok(Json(updated))
}
