use axum::{extract::Request, http::header, middleware::Next, response::Response};

/// Menambahkan `Cache-Control: no-store` pada respons endpoint transaksi
/// (RULES §9 RL-13) agar data pembayaran tidak tersimpan di cache perantara.
/// Mencakup endpoint di bawah `/api/v1/transactions/...` maupun pembuatan
/// transaksi di `/api/v1/merchants/{id}/transactions`.
pub async fn no_store_transactions(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let is_transaction_path =
        path.starts_with("/api/v1/transactions") || path.ends_with("/transactions");

    let mut response = next.run(req).await;

    if is_transaction_path {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
    }

    response
}
