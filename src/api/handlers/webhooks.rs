use crate::auth::crypto::decrypt_secret;
use crate::error::AppError;
use crate::services::matcher::MatcherService;
use crate::services::parser::{PaymentDirection, parse_notification_text};
use crate::state::AppState;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

#[derive(sqlx::FromRow)]
struct SourceLookup {
    id: String,
    merchant_id: String,
    channel: String,
    secret_enc: String,
    status: String,
    merchant_status: String,
}

/// POST /api/v1/webhooks/mutation/{source_id}
pub async fn mutation_webhook_handler(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    body_bytes: Bytes,
) -> Result<impl IntoResponse, AppError> {
    // 1. Ekstraksi Header Wajib (WH-02, WH-03, ERP §9.8)
    let timestamp_str = headers
        .get("X-Qris-Timestamp")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Internal("Header X-Qris-Timestamp wajib".to_string()))?;

    let event_id_header = headers
        .get("X-Qris-Event-Id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Internal("Header X-Qris-Event-Id wajib".to_string()))?;

    let signature_header = headers
        .get("X-Qris-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Internal("Header X-Qris-Signature wajib".to_string()))?;

    // 2. Lookup Source di Database
    let source = sqlx::query_as::<_, SourceLookup>(
        r#"
        SELECT s.id, s.merchant_id, s.channel, s.secret_enc, s.status,
               m.status as merchant_status
        FROM notification_sources s
        JOIN merchants m ON m.id = s.merchant_id
        WHERE s.id = ?
        "#,
    )
    .bind(&source_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .ok_or_else(|| AppError::Internal("Notification source tidak ditemukan".to_string()))?;

    if source.status != "ACTIVE" || source.merchant_status != "ACTIVE" {
        return Err(AppError::MerchantInactive);
    }

    // 3. Verifikasi Signature HMAC SEBELUM Body di-parse (WH-02)
    let aad = format!("notification_sources:{}:{}", source.id, source.merchant_id);
    let source_secret =
        decrypt_secret(&source.secret_enc, &state.config.data_encryption_key, &aad)?;

    let mut body_hasher = Sha256::new();
    body_hasher.update(&body_bytes);
    let body_hash_hex = format!("{:x}", body_hasher.finalize());

    // Kanonik: timestamp + "\n" + event_id + "\n" + body_hash_hex
    let canonical = format!("{}\n{}\n{}", timestamp_str, event_id_header, body_hash_hex);

    let mut mac = HmacSha256::new_from_slice(source_secret.as_bytes())
        .map_err(|e| AppError::Internal(e.to_string()))?;
    mac.update(canonical.as_bytes());
    let expected_signature = format!("v1={:x}", mac.finalize().into_bytes());

    let is_valid: bool = expected_signature
        .as_bytes()
        .ct_eq(signature_header.as_bytes())
        .into();
    if !is_valid {
        return Err(AppError::AuthInvalid);
    }

    // 4. Parse Body Sesuai Kanal (Android, Email, Aggregator)
    let json_val: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| AppError::Internal(format!("Body bukan JSON valid: {}", e)))?;

    let (parsed_amount, parsed_direction, raw_text_masked, parser_version, match_reason) =
        match source.channel.as_str() {
            "AGGREGATOR" => {
                // Nominal terstruktur dari Aggregator (Moota / CekMutasi)
                let amt = json_val.get("amount").and_then(|v| v.as_i64());
                let dir = json_val
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("CR");
                let desc = json_val
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                (
                    amt,
                    dir,
                    desc.to_string(),
                    "aggregator_v1",
                    "structured_aggregator",
                )
            }
            _ => {
                // Android forwarder atau Email forwarder
                let text = json_val.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let parsed = parse_notification_text(text);
                let dir_str = match parsed.direction {
                    PaymentDirection::Credit => "CR",
                    PaymentDirection::Debit => "DR",
                    PaymentDirection::Unknown => "UNKNOWN",
                };
                (
                    parsed.amount,
                    dir_str,
                    parsed.masked_text,
                    parsed.parser_version,
                    parsed.match_reason,
                )
            }
        };

    // 5. Eksekusi Exact Matcher
    let matcher = MatcherService::new(&state.db);
    let res = matcher
        .match_payment(crate::services::matcher::MatchEventParams {
            source_id: &source.id,
            merchant_id: &source.merchant_id,
            channel: &source.channel,
            event_key: event_id_header,
            raw_payload_sha256: &body_hash_hex,
            raw_text_masked: Some(&raw_text_masked),
            parsed_amount,
            parsed_direction,
            parser_version,
            match_reason,
        })
        .await?;

    Ok((StatusCode::ACCEPTED, Json(res)))
}
