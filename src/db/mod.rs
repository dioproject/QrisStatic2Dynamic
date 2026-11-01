pub mod backup;

use sqlx::{
    Pool, Sqlite,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::path::Path;
use std::str::FromStr;
use tracing::info;

pub type DbPool = Pool<Sqlite>;

/// Membuka satu file SQLite langsung (dipakai verifikasi backup).
pub async fn connect_file(path: &Path, read_only: bool) -> Result<DbPool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(read_only)
        .create_if_missing(false);

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

/// Membuka pool SQLite dengan pragma standar aplikasi.
///
/// `create_if_missing = false` dipakai oleh perintah operasional (backup,
/// verify, cleanup) agar salah path tidak diam-diam membuat database kosong.
pub async fn connect_db(
    database_url: &str,
    create_if_missing: bool,
) -> Result<DbPool, sqlx::Error> {
    let connection_options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(create_if_missing)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .min_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect_with(connection_options)
        .await?;

    info!("Database SQLite terhubung: {}", database_url);

    Ok(pool)
}

pub async fn init_db(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let pool = connect_db(database_url, true).await?;

    // Jalankan migrasi otomatis
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("Migrasi skema database berhasil diterapkan.");

    Ok(pool)
}
