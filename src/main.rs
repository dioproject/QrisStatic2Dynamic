use qris_api::db::init_db;
use qris_api::services::outbound::OutboundWorker;
use qris_api::services::sweeper::SweeperWorker;
use qris_api::{AppConfig, AppState, build_router};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = AppConfig::load_from_env()?;
    let bind_addr = config.app_bind.clone();
    let dek_b64 = config.data_encryption_key.clone();
    let allow_insecure_callbacks = config.allow_insecure_callbacks;

    // Inisialisasi pool database dan jalankan migrasi
    let db = init_db(&config.database_url).await?;

    // Sweeper: expiry transaksi (TX-04) + purge nonce (DR-04)
    let sweeper = SweeperWorker::new(db.clone(), 30, 300);
    tokio::spawn(async move {
        sweeper.run().await;
    });

    // Outbound outbox: kirim callback merchant dengan retry/backoff (OW-04)
    let outbound = OutboundWorker::new(db.clone(), dek_b64, 5, allow_insecure_callbacks);
    tokio::spawn(async move {
        outbound.run().await;
    });

    let state = AppState::new(config, db);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Server QRIS berjalan pada: http://{}", bind_addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    info!("Server berhenti dengan rapi.");
    Ok(())
}

/// Menunggu Ctrl+C atau SIGTERM agar koneksi yang berjalan tidak diputus paksa.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
