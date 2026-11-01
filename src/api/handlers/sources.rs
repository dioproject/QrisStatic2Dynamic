use crate::auth::RequireMaster;
use crate::auth::crypto::{encrypt_secret, generate_token};
use crate::error::AppError;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Deserialize)]
pub struct CreateSourcePayload {
    pub channel: String,
    pub label: String,
    #[serde(default = "default_config")]
    pub config_json: serde_json::Value,
}

fn default_config() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Serialize)]
pub struct CreateSourceResponse {
    pub id: String,
    pub channel: String,
    pub label: String,
    pub webhook_url: String,
    pub source_secret: String,
    pub note: &'static str,
}

/// POST /api/v1/merchants/{id}/sources (Master only)
pub async fn create_source_handler(
    State(state): State<AppState>,
    _master: RequireMaster,
    Path(merchant_id): Path<String>,
    Json(payload): Json<CreateSourcePayload>,
) -> Result<impl IntoResponse, AppError> {
    let channel_upper = payload.channel.to_uppercase();
    if !["ANDROID", "EMAIL", "AGGREGATOR", "CUSTOM"].contains(&channel_upper.as_str()) {
        return Err(AppError::Internal(
            "Kanal notification source tidak didukung".to_string(),
        ));
    }

    let source_id = format!("src_{}", Ulid::new());
    let source_secret = generate_token("qws_live_", 40);
    let secret_prefix = source_secret[..16].to_string();
    let aad = format!("notification_sources:{}:{}", source_id, merchant_id);
    let secret_enc = encrypt_secret(&source_secret, &state.config.data_encryption_key, &aad)?;

    let now = chrono::Utc::now().timestamp_millis();
    let config_str = payload.config_json.to_string();

    sqlx::query(
        r#"
        INSERT INTO notification_sources (
            id, merchant_id, channel, label, secret_prefix, secret_enc, config_json, status, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 'ACTIVE', ?)
        "#,
    )
    .bind(&source_id)
    .bind(&merchant_id)
    .bind(&channel_upper)
    .bind(&payload.label)
    .bind(&secret_prefix)
    .bind(&secret_enc)
    .bind(config_str)
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let webhook_url = format!(
        "{}/api/v1/webhooks/mutation/{}",
        state.config.app_base_url, source_id
    );

    let _ = crate::services::audit::AuditLogger::log(
        &state.db,
        &crate::auth::Principal::Master,
        "merchant.source.create",
        "source",
        &source_id,
        Some(&merchant_id),
        None,
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(CreateSourceResponse {
            id: source_id,
            channel: channel_upper,
            label: payload.label,
            webhook_url,
            source_secret,
            note: "Simpan sekarang. Source secret tidak akan ditampilkan lagi.",
        }),
    ))
}
