use crate::telemetry;
use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue, header},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};
use std::time::Instant;
use tracing::info;

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Header `X-Request-Id` yang tervalidasi; dapat diekstrak handler.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

/// Memberi setiap request `X-Request-Id` (OP-08, ERP §8.1) dan mencatat
/// satu baris log terstruktur setelah respons selesai.
///
/// ID dari klien dipakai kembali hanya bila formatnya aman (8–64 karakter
/// alfanumerik/`_`/`-`), sehingga bisa dipakai untuk korelasi lintas layanan
/// tanpa membuka celah header injection.
pub async fn request_id(mut req: Request, next: Next) -> Response {
    let id = resolve_request_id(&req);
    req.extensions_mut().insert(RequestId(id.clone()));

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    // Fingerprint kredensial (bukan token mentah) sebagai principal_id log.
    let principal_id = credential_fingerprint(&req);
    let started = Instant::now();

    let mut response = telemetry::REQUEST_ID.scope(id.clone(), next.run(req)).await;

    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }

    info!(
        request_id = %id,
        method = %method,
        path = %path,
        status = response.status().as_u16(),
        latency_ms = started.elapsed().as_millis() as u64,
        principal_id = %principal_id,
        "request selesai"
    );

    response
}

fn resolve_request_id(req: &Request) -> String {
    if let Some(candidate) = req
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        && (8..=64).contains(&candidate.len())
        && candidate
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return candidate.to_string();
    }

    format!("req_{}", ulid::Ulid::new())
}

fn credential_fingerprint(req: &Request) -> String {
    let Some(token) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return "-".to_string();
    };

    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("cred_{}", &digest[..16])
}
