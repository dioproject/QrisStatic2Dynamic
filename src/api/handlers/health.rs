use axum::{Json, response::IntoResponse};
use serde_json::json;

pub async fn healthz() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

pub async fn readyz() -> impl IntoResponse {
    Json(json!({
        "status": "ready"
    }))
}
