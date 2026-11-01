use qris_api::db::init_db;
use qris_api::{AppConfig, AppState, build_router};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = AppConfig::load_from_env()?;
    let bind_addr = config.app_bind.clone();

    // Inisialisasi pool database dan jalankan migrasi
    let db = init_db(&config.database_url).await?;

    // Jalankan Sweeper Worker di background (TX-04 & DR-04)
    let sweeper = qris_api::services::sweeper::SweeperWorker::new(db.clone(), 30, 300);
    tokio::spawn(async move {
        sweeper.run().await;
    });

    let state = AppState::new(config, db);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Server QRIS berjalan pada: http://{}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}
