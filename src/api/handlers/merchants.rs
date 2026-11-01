use crate::auth::{Principal, RequireAuth, RequireMaster};
use crate::error::AppError;
use crate::services::merchant_service::MerchantService;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CreateMerchantPayload {
    pub name: String,
    pub static_payload: String,
    #[serde(default = "default_fee_mode")]
    pub default_fee_mode: String,
    #[serde(default)]
    pub default_fee_fixed: i64,
    #[serde(default)]
    pub default_fee_percent_bps: i32,
    #[serde(default = "default_true")]
    pub default_unique_code: bool,
    #[serde(default = "default_ttl")]
    pub default_ttl_seconds: i64,
    #[serde(default)]
    pub callback_url: Option<String>,
}

fn default_fee_mode() -> String {
    "NONE".to_string()
}
fn default_true() -> bool {
    true
}
fn default_ttl() -> i64 {
    900
}

#[derive(Debug, Deserialize)]
pub struct ListMerchantsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
pub struct ListMerchantsResponse {
    pub items: Vec<crate::services::merchant_service::Merchant>,
}

/// POST /api/v1/merchants (Master only)
pub async fn create_merchant_handler(
    State(state): State<AppState>,
    _master: RequireMaster,
    Json(payload): Json<CreateMerchantPayload>,
) -> Result<impl IntoResponse, AppError> {
    // Callback URL wajib HTTPS publik (RULES §9 RL-14, OW-12).
    // Divalidasi di boundary HTTP agar config dev dapat mengizinkan localhost.
    if let Some(cb) = payload.callback_url.as_deref() {
        crate::services::outbound::validate_callback_url(
            cb,
            state.config.allow_insecure_callbacks,
        )?;
    }

    let service = MerchantService::new(&state.db, &state.config.data_encryption_key);
    let res = service
        .create_merchant(
            &payload.name,
            &payload.static_payload,
            &payload.default_fee_mode,
            payload.default_fee_fixed,
            payload.default_fee_percent_bps,
            payload.default_unique_code,
            payload.default_ttl_seconds,
            payload.callback_url.as_deref(),
        )
        .await?;

    let _ = crate::services::audit::AuditLogger::log(
        &state.db,
        &crate::auth::Principal::Master,
        "merchant.create",
        "merchant",
        &res.merchant.id,
        Some(&res.merchant.id),
        None,
    )
    .await;

    Ok((StatusCode::CREATED, Json(res)))
}

/// GET /api/v1/merchants (Master only)
pub async fn list_merchants_handler(
    State(state): State<AppState>,
    _master: RequireMaster,
    Query(query): Query<ListMerchantsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let service = MerchantService::new(&state.db, &state.config.data_encryption_key);
    let items = service.list_merchants(query.limit).await?;
    Ok(Json(ListMerchantsResponse { items }))
}

/// GET /api/v1/merchants/{id} (Master or Merchant Owner)
pub async fn get_merchant_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    if let Principal::Merchant { merchant_id, .. } = &principal
        && merchant_id != &id
    {
        return Err(AppError::AuthForbidden);
    }

    let service = MerchantService::new(&state.db, &state.config.data_encryption_key);
    let merchant = service.get_merchant(&id).await?;
    Ok(Json(merchant))
}

#[derive(Debug, Serialize)]
pub struct TestWebhookResponse {
    pub delivery_id: String,
    pub merchant_id: String,
    pub event_type: &'static str,
    pub url: String,
    pub status: &'static str,
}

/// POST /api/v1/merchants/{id}/test-webhook (Master only)
pub async fn test_webhook_handler(
    State(state): State<AppState>,
    _master: RequireMaster,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let service = MerchantService::new(&state.db, &state.config.data_encryption_key);
    let merchant = service.get_merchant(&id).await?;

    let callback_url = merchant.callback_url.ok_or_else(|| {
        AppError::ValidationError("Merchant belum memiliki callback_url.".to_string())
    })?;

    let delivery_id = format!("dlv_{}", ulid::Ulid::new());
    let now = chrono::Utc::now().timestamp_millis();
    let payload = serde_json::json!({
        "event": "merchant.test",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "data": {
            "merchant_id": merchant.id,
            "merchant_name": merchant.merchant_name,
            "message": "Uji coba webhook merchant berhasil dikirim."
        }
    });

    sqlx::query(
        r#"
        INSERT INTO webhook_deliveries (
            id, merchant_id, transaction_id, event_type, url, payload_json,
            status, attempts, next_attempt_at, created_at
        )
        VALUES (?, ?, NULL, 'merchant.test', ?, ?, 'PENDING', 0, ?, ?)
        "#,
    )
    .bind(&delivery_id)
    .bind(&merchant.id)
    .bind(&callback_url)
    .bind(payload.to_string())
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let _ = crate::services::audit::AuditLogger::log(
        &state.db,
        &crate::auth::Principal::Master,
        "merchant.test_webhook",
        "delivery",
        &delivery_id,
        Some(&merchant.id),
        None,
    )
    .await;

    Ok((
        StatusCode::ACCEPTED,
        Json(TestWebhookResponse {
            delivery_id,
            merchant_id: merchant.id,
            event_type: "merchant.test",
            url: callback_url,
            status: "PENDING",
        }),
    ))
}
