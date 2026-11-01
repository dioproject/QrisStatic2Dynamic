use crate::error::{ErrorBody, ErrorEnvelope};
use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;

/// Endpoint yang melakukan decode gambar (kuota lebih ketat, RL-03).
const IMAGE_PATH: &str = "/api/v1/qris/parse";

/// Middleware rate limit per kredensial dan per IP (RL-01 s/d RL-04).
pub async fn rate_limit(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();

    // Health check tidak dibatasi agar probe tidak pernah gagal karena kuota.
    if path == "/healthz" || path == "/readyz" {
        return next.run(req).await;
    }

    // 1. Kunci kredensial: hash token (jangan simpan token mentah di memori key).
    let cred_key = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|token| {
            let mut hasher = Sha256::new();
            hasher.update(token.as_bytes());
            let digest = format!("{:x}", hasher.finalize());
            format!("cred:{}", &digest[..16])
        });

    // 2. Kunci IP: utamakan koneksi TCP (`ConnectInfo`). Header
    //    `X-Forwarded-For` hanya dipercaya bila `TRUST_PROXY_HEADERS=true`,
    //    sehingga klien tidak bisa memalsukannya untuk melewati RL-01.
    let ip_key = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|ci| format!("ip:{}", ci.0.ip()))
        .or_else(|| {
            if !state.config.trust_proxy_headers {
                return None;
            }
            req.headers()
                .get("X-Forwarded-For")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
                .map(|ip| format!("ip:{}", ip.trim()))
        })
        .unwrap_or_else(|| "ip:unknown".to_string());

    // 3. Evaluasi limit umum.
    if let Some(key) = cred_key.as_deref()
        && let Some(resp) = reject_if_limited(state.rate_limiter.check(key))
    {
        return resp;
    }
    if let Some(resp) = reject_if_limited(state.rate_limiter.check(&ip_key)) {
        return resp;
    }

    // 4. Limit tambahan untuk decode gambar.
    if path == IMAGE_PATH && req.method() == axum::http::Method::POST {
        let image_key = format!("img:{}", cred_key.as_deref().unwrap_or(&ip_key));
        if let Some(resp) = reject_if_limited(state.rate_limiter.check_image(&image_key)) {
            return resp;
        }
    }

    next.run(req).await
}

fn reject_if_limited(decision: crate::services::rate_limit::RateLimitDecision) -> Option<Response> {
    if decision.allowed {
        return None;
    }

    let body = ErrorEnvelope {
        error: ErrorBody {
            code: "RATE_LIMITED".to_string(),
            message: "Terlalu banyak permintaan. Silakan coba lagi.".to_string(),
            details: None,
            request_id: None,
        },
    };

    Some(
        (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                header::RETRY_AFTER,
                decision.retry_after_secs.max(1).to_string(),
            )],
            axum::Json(body),
        )
            .into_response(),
    )
}
