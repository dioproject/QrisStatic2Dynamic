use crate::auth::hmac::{RequestSignatureParams, SignedRequestValidator};
use crate::auth::resolve_principal;
use crate::error::AppError;
use crate::state::AppState;
use axum::{
    body::Body,
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Batas body JSON (RULES §9 RL-05).
const MAX_JSON_BODY: usize = 256 * 1024;

/// Middleware signed request (ERP §8.3).
///
/// Berlaku untuk semua request non-GET ber-autentikasi ketika
/// `REQUIRE_SIGNED_REQUESTS=true`. Body di-buffer agar dapat diverifikasi
/// lalu diteruskan utuh ke handler.
pub async fn require_signed_request(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    match enforce(&state, req, next).await {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

async fn enforce(state: &AppState, req: Request, next: Next) -> Result<Response, AppError> {
    // SG-15: GET/HEAD/OPTIONS tidak wajib signature.
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path().to_string();

    // Endpoint publik (health) dan inbound webhook (punya HMAC source sendiri,
    // kanonik berbeda — RULES §9.1 WH-04) tidak melewati validator ini.
    if path == "/healthz" || path == "/readyz" || path.starts_with("/api/v1/webhooks/") {
        return Ok(next.run(req).await);
    }

    // Mode non-strict (dev): signature tidak diwajibkan.
    if !state.config.require_signed_requests {
        return Ok(next.run(req).await);
    }

    // 1. Resolusi principal dari Bearer token.
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .ok_or(AppError::AuthMissing)?
        .to_string();

    let principal = resolve_principal(state, &token).await?;

    // 2. Buffer body. Melebihi batas → 413 tanpa membaca seluruh body.
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_JSON_BODY)
        .await
        .map_err(|_| AppError::PayloadTooLarge)?;

    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let method = parts.method.as_str().to_string();

    let header_value = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };

    let ts = header_value("X-Qris-Timestamp");
    let nonce = header_value("X-Qris-Nonce");
    let sig = header_value("X-Qris-Signature");

    // 3. Verifikasi signature + nonce anti-replay.
    SignedRequestValidator::validate(
        state,
        &principal,
        RequestSignatureParams {
            method: &method,
            path_and_query: &path_and_query,
            raw_body: &bytes,
            timestamp_str: ts.as_deref(),
            nonce_str: nonce.as_deref(),
            signature_str: sig.as_deref(),
        },
    )
    .await?;

    // 4. Teruskan request dengan body utuh.
    let req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(req).await)
}
