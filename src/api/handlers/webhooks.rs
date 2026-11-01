use crate::error::AppError;
use crate::services::matcher::{MatchEventParams, MatcherService};
use crate::services::parser::{PaymentDirection, mask_text, parse_notification_text};
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

/// Batas body inbound webhook (RULES §9.1 WH-06).
const MAX_WEBHOOK_BODY: usize = 64 * 1024;
/// Toleransi clock skew signature (WH-05).
const SIGNATURE_SKEW_SECONDS: i64 = 300;
/// `last_seen_at` diperbarui maksimal sekali per menit (WH-13).
const LAST_SEEN_THROTTLE_MS: i64 = 60_000;

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
    // 0. Batas ukuran body (WH-06).
    if body_bytes.len() > MAX_WEBHOOK_BODY {
        return Err(AppError::PayloadTooLarge);
    }

    // 1. Header wajib (WH-03). Header hilang = 401/400, BUKAN 500.
    let timestamp_str = headers
        .get("X-Qris-Timestamp")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::SignatureRequired)?;

    let event_id_header = headers
        .get("X-Qris-Event-Id")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::WebhookEventIdRequired)?;

    let signature_header = headers
        .get("X-Qris-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::SignatureRequired)?;

    // WH-03: event id 1-128 karakter.
    if event_id_header.is_empty() || event_id_header.len() > 128 {
        return Err(AppError::WebhookEventIdInvalid);
    }

    // 2. Skew timestamp (WH-05).
    let req_ts: i64 = timestamp_str
        .parse()
        .map_err(|_| AppError::SignatureTimestampInvalid)?;
    if (chrono::Utc::now().timestamp() - req_ts).abs() > SIGNATURE_SKEW_SECONDS {
        return Err(AppError::SignatureTimestampInvalid);
    }

    // 3. Lookup source (WH-01).
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
    .ok_or(AppError::SourceNotFound)?;

    if source.status != "ACTIVE" {
        return Err(AppError::SourceRevoked);
    }
    if source.merchant_status != "ACTIVE" {
        return Err(AppError::MerchantInactive);
    }

    // 4. Verifikasi signature SEBELUM body di-parse (WH-02, WH-04).
    let aad = format!("notification_sources:{}:{}", source.id, source.merchant_id);
    let source_secret = crate::auth::crypto::decrypt_secret(
        &source.secret_enc,
        &state.config.data_encryption_key,
        &aad,
    )?;

    let mut body_hasher = Sha256::new();
    body_hasher.update(&body_bytes);
    let body_hash_hex = format!("{:x}", body_hasher.finalize());

    // Kanonik inbound: timestamp + "\n" + event_id + "\n" + sha256(raw_body)
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
        return Err(AppError::SignatureInvalid);
    }

    // 5. Catat source terlihat (WH-13, maksimal sekali per menit).
    let now_ms = chrono::Utc::now().timestamp_millis();
    let _ = sqlx::query(
        r#"
        UPDATE notification_sources
        SET last_seen_at = ?
        WHERE id = ? AND (last_seen_at IS NULL OR last_seen_at < ?)
        "#,
    )
    .bind(now_ms)
    .bind(&source.id)
    .bind(now_ms - LAST_SEEN_THROTTLE_MS)
    .execute(&state.db)
    .await;

    // 6. Parse body sesuai kanal. Body rusak → 422, bukan 500 (WH-15).
    let json_val: Value =
        serde_json::from_slice(&body_bytes).map_err(|_| AppError::WebhookBodyInvalid)?;

    let (parsed_amount, parsed_direction, raw_text_masked, parser_version, match_reason) =
        match source.channel.as_str() {
            "AGGREGATOR" => {
                // Nominal terstruktur, JANGAN diubah kecuali validasi range (PR-11).
                let amt = json_val
                    .get("amount")
                    .and_then(|v| v.as_i64())
                    .filter(|v| (1..=1_000_000_000_000).contains(v));
                let dir = json_val
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("CR")
                    .to_uppercase();
                let dir = match dir.as_str() {
                    "CR" | "DR" => dir,
                    _ => "UNKNOWN".to_string(),
                };
                let desc = json_val
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                (
                    amt,
                    dir,
                    mask_text(desc),
                    "aggregator_v1",
                    "structured_aggregator",
                )
            }
            _ => {
                // Android forwarder atau Email forwarder.
                let text = json_val.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let parsed = parse_notification_text(text);
                let dir_str = match parsed.direction {
                    PaymentDirection::Credit => "CR",
                    PaymentDirection::Debit => "DR",
                    PaymentDirection::Unknown => "UNKNOWN",
                };
                (
                    parsed.amount,
                    dir_str.to_string(),
                    parsed.masked_text,
                    parsed.parser_version,
                    parsed.match_reason,
                )
            }
        };

    // 7. Eksekusi exact matcher (MT-01 s/d MT-14).
    let matcher = MatcherService::new(&state.db);
    let res = matcher
        .match_payment(MatchEventParams {
            source_id: &source.id,
            merchant_id: &source.merchant_id,
            channel: &source.channel,
            event_key: event_id_header,
            raw_payload_sha256: &body_hash_hex,
            raw_text_masked: Some(&raw_text_masked),
            parsed_amount,
            parsed_direction: &parsed_direction,
            parser_version,
            match_reason,
        })
        .await?;

    // WH-10: duplikat tetap 202 tanpa efek samping.
    Ok((StatusCode::ACCEPTED, Json(res)))
}
