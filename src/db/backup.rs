use std::path::Path;

/// Membuat backup konsisten memakai `VACUUM INTO` lalu memverifikasi hasilnya
/// dengan `PRAGMA integrity_check` (ERP §11.2, RULES OP-04).
///
/// Backup tidak pernah menimpa file yang sudah ada: lebih aman gagal daripada
/// menimpa backup sebelumnya secara tidak sengaja.
pub async fn backup_database(database_url: &str, out: &Path) -> Result<(), String> {
    if out.exists() {
        return Err(format!(
            "File backup sudah ada, tidak ditimpa: {}",
            out.display()
        ));
    }

    let out_str = out
        .to_str()
        .ok_or_else(|| "Path backup bukan UTF-8 yang valid.".to_string())?;

    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Gagal membuat direktori backup {}: {e}", parent.display()))?;
    }

    let pool = super::connect_db(database_url, false)
        .await
        .map_err(|e| format!("Gagal membuka database sumber: {e}"))?;

    // `VACUUM INTO` menghasilkan snapshot konsisten tanpa mengunci pembaca lama.
    let result = sqlx::query("VACUUM INTO ?")
        .bind(out_str)
        .execute(&pool)
        .await;
    pool.close().await;
    result.map_err(|e| format!("VACUUM INTO gagal: {e}"))?;

    let integrity = integrity_check_file(out).await?;
    if integrity != "ok" {
        return Err(format!(
            "Verifikasi backup gagal (integrity_check): {integrity}"
        ));
    }

    Ok(())
}

/// Menjalankan `PRAGMA integrity_check` pada file database yang sudah ada.
pub async fn integrity_check_file(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("Database tidak ditemukan: {}", path.display()));
    }

    let pool = super::connect_file(path, true)
        .await
        .map_err(|e| format!("Gagal membuka database {}: {e}", path.display()))?;

    let result: Result<String, sqlx::Error> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await;
    pool.close().await;

    result.map_err(|e| format!("integrity_check gagal: {e}"))
}

/// Menjalankan `PRAGMA integrity_check` pada database yang dibuka lewat URL.
pub async fn integrity_check_url(database_url: &str) -> Result<String, String> {
    let pool = super::connect_db(database_url, false)
        .await
        .map_err(|e| format!("Gagal membuka database: {e}"))?;

    let result: Result<String, sqlx::Error> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await;
    pool.close().await;

    result.map_err(|e| format!("integrity_check gagal: {e}"))
}
