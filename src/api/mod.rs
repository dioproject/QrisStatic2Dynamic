pub mod handlers;
pub mod middleware;

use crate::state::AppState;
use axum::{
    Router, middleware as axum_middleware,
    routing::{get, post},
};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        // Stateless QRIS
        .route("/api/v1/qris/parse", post(handlers::qris::parse_handler))
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
        )
        // Signed request (HMAC + nonce anti-replay) untuk semua non-GET ber-auth.
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::signed::require_signed_request,
        ))
        .with_state(state)
}
