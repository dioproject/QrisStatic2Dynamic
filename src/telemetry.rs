use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

tokio::task_local! {
    /// Request ID aktif pada task yang sedang menangani request (OP-08).
    pub static REQUEST_ID: String;
}

/// Request ID untuk task saat ini, bila dipanggil di dalam scope middleware.
pub fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

/// Inisialisasi tracing.
///
/// Produksi memakai JSON agar field terstruktur mudah diolah; development
/// memakai format pretty. Level diatur lewat `RUST_LOG` (default `info`).
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let is_production = std::env::var("APP_ENV")
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false);

    let registry = tracing_subscriber::registry().with(filter);

    if is_production {
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
    }
}
