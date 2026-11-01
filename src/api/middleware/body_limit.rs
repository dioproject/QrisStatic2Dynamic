use crate::error::AppError;
use axum::{
    extract::Request,
    http::header,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Batas body JSON (RULES §9 RL-05).
const MAX_JSON_BODY: u64 = 256 * 1024;

/// Menolak lebih awal body JSON yang jelas melebihi batas (RL-06).
///
/// `DefaultBodyLimit` pada route `/qris/parse` sengaja lebih besar karena
/// endpoint yang sama menerima multipart 2 MB. Middleware ini memeriksa
/// `Content-Length` sebelum body dibaca, sehingga body JSON raksasa tidak
/// pernah dialokasikan. Body tanpa `Content-Length` (chunked) tetap dibatasi
/// oleh `DefaultBodyLimit` dan pemeriksaan ukuran di handler.
pub async fn qris_body_limit(req: Request, next: Next) -> Response {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if content_type.starts_with("application/json")
        && let Some(len) = req
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        && len > MAX_JSON_BODY
    {
        return AppError::PayloadTooLarge.into_response();
    }

    next.run(req).await
}
