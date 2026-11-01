pub mod handlers;
pub mod middleware;

use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderName, HeaderValue, Method, header},
    middleware as axum_middleware,
    routing::{get, post},
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::warn;

/// Batas body JSON (RULES §9 RL-05).
const JSON_BODY_LIMIT: usize = 256 * 1024;
/// Batas body multipart/upload (RL-05). Ada kelonggaran untuk overhead multipart.
const MULTIPART_BODY_LIMIT: usize = 2 * 1024 * 1024 + 64 * 1024;

pub fn build_router(state: AppState) -> Router {
    let cors = build_cors_layer(&state.config.cors_allowed_origins);

    let router = Router::new()
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        // Stateless QRIS
        .route(
            "/api/v1/qris/parse",
            // Endpoint ini menerima upload gambar → batas body lebih besar.
            // Middleware JSON limit menjaga body JSON tetap 256 KB (RL-06).
            post(handlers::qris::parse_handler)
                .layer(DefaultBodyLimit::max(MULTIPART_BODY_LIMIT))
                .layer(axum_middleware::from_fn(
                    middleware::body_limit::qris_body_limit,
                )),
        )
        .route(
            "/api/v1/qris/dynamic",
            post(handlers::qris::dynamic_handler),
        )
        // Stateful Merchants & Sources
        .route(
            "/api/v1/merchants",
            post(handlers::merchants::create_merchant_handler)
                .get(handlers::merchants::list_merchants_handler),
        )
        .route(
            "/api/v1/merchants/{id}",
            get(handlers::merchants::get_merchant_handler),
        )
        .route(
            "/api/v1/merchants/{id}/sources",
            post(handlers::sources::create_source_handler),
        )
        // Stateful Transactions
        .route(
            "/api/v1/merchants/{id}/transactions",
            post(handlers::transactions::create_transaction_handler),
        )
        .route(
            "/api/v1/transactions/{id}",
            get(handlers::transactions::get_transaction_handler),
        )
        .route(
            "/api/v1/transactions/{id}/status",
            get(handlers::transactions::get_transaction_status_handler),
        )
        .route(
            "/api/v1/transactions/{id}/stream",
            get(handlers::stream::transaction_stream_handler),
        )
        .route(
            "/api/v1/transactions/{id}/qr.png",
            get(handlers::transactions::get_transaction_png_handler),
        )
        .route(
            "/api/v1/transactions/{id}/qr.svg",
            get(handlers::transactions::get_transaction_svg_handler),
        )
        .route(
            "/api/v1/transactions/{id}/cancel",
            post(handlers::transactions::cancel_transaction_handler),
        )
        // Inbound Webhooks Mutasi (HMAC source secret)
        .route(
            "/api/v1/webhooks/mutation/{source_id}",
            post(handlers::webhooks::mutation_webhook_handler),
        );

    router
        // `Cache-Control: no-store` untuk respons transaksi (RL-13).
        .layer(axum_middleware::from_fn(
            middleware::cache::no_store_transactions,
        ))
        // Signed request: HMAC + nonce anti-replay (ERP §8.3).
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::signed::require_signed_request,
        ))
        // Rate limit per kredensial + IP (RL-01..RL-04).
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::rate_limit::rate_limit,
        ))
        // Header keamanan (RL-13).
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        // CORS allowlist; kosong = tidak ada origin yang diizinkan (RL-12).
        .layer(cors)
        // Batas body default untuk semua route (RL-05).
        .layer(DefaultBodyLimit::max(JSON_BODY_LIMIT))
        // Request ID + log terstruktur (OP-08); paling luar agar semua
        // respons (termasuk 4xx/5xx) memuat header dan tercatat.
        .layer(axum_middleware::from_fn(middleware::request_id::request_id))
        .with_state(state)
}

fn build_cors_layer(origins: &[String]) -> CorsLayer {
    let mut layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            HeaderName::from_static("idempotency-key"),
            HeaderName::from_static("x-qris-timestamp"),
            HeaderName::from_static("x-qris-nonce"),
            HeaderName::from_static("x-qris-signature"),
        ]);

    if origins.is_empty() {
        // Default deny: tanpa allow_origin, browser tidak akan menerima header CORS.
        return layer;
    }

    let parsed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|origin| match HeaderValue::from_str(origin) {
            Ok(v) => Some(v),
            Err(_) => {
                warn!("Origin CORS diabaikan karena tidak valid: {}", origin);
                None
            }
        })
        .collect();

    if parsed.is_empty() {
        return layer;
    }

    layer = layer.allow_origin(AllowOrigin::list(parsed));
    layer
}
