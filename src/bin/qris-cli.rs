//! CLI operasional: migrate, backup, verify, cleanup (RULES OP-04/OP-05).
//!
//! Contoh:
//! ```text
//! qris-cli migrate
//! qris-cli backup --out /backup/qris-20261101.db
//! qris-cli verify
//! qris-cli cleanup
//! ```

use qris_api::db::{backup, connect_db, init_db};
use qris_api::services::sweeper::SweeperWorker;

const USAGE: &str = "\
qris-cli — utilitas operasional QrisStatic2Dynamic

Pemakaian:
  qris-cli migrate                 Terapkan migrasi yang tertunda
  qris-cli backup --out <path>     Backup konsisten (VACUUM INTO) + integrity_check
  qris-cli verify                  Jalankan PRAGMA integrity_check
  qris-cli cleanup                 Sapu transaksi kedaluwarsa, nonce, dan retensi data

Database diambil dari variabel lingkungan DATABASE_URL.
";

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    qris_api::telemetry::init();

    if let Err(err) = run().await {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = match args.next() {
        Some(cmd) => cmd,
        None => {
            print!("{USAGE}");
            return Err("Perintah wajib diisi.".to_string());
        }
    };

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://./data/qris.db".to_string());

    match command.as_str() {
        "migrate" => {
            let pool = init_db(&database_url)
                .await
                .map_err(|e| format!("Migrasi gagal: {e}"))?;
            pool.close().await;
            println!("Migrasi selesai.");
            Ok(())
        }
        "backup" => {
            let out = parse_out_path(&mut args)?;
            backup::backup_database(&database_url, std::path::Path::new(&out)).await?;
            println!("Backup terverifikasi: {out}");
            Ok(())
        }
        "verify" => {
            let result = backup::integrity_check_url(&database_url).await?;
            if result != "ok" {
                return Err(format!("integrity_check gagal: {result}"));
            }
            println!("integrity_check: ok");
            Ok(())
        }
        "cleanup" => {
            let pool = connect_db(&database_url, false)
                .await
                .map_err(|e| format!("Gagal membuka database: {e}"))?;

            let worker = SweeperWorker::new(pool.clone(), 3600, 300);
            let expired = worker
                .sweep_expired_transactions()
                .await
                .map_err(|e| format!("Gagal menyapu transaksi kedaluwarsa: {e}"))?;
            let nonces = worker
                .cleanup_old_nonces()
                .await
                .map_err(|e| format!("Gagal membersihkan nonce: {e}"))?;
            let (events, deliveries) = worker
                .run_retention()
                .await
                .map_err(|e| format!("Retensi gagal: {e}"))?;

            pool.close().await;
            println!(
                "Cleanup selesai: {expired} transaksi EXPIRED, {nonces} nonce, \
                 {events} event, {deliveries} delivery dihapus."
            );
            Ok(())
        }
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("Perintah tidak dikenal: {other}")),
    }
}

fn parse_out_path(args: &mut impl Iterator<Item = String>) -> Result<String, String> {
    let mut out = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = args.next();
            }
            _ if arg.starts_with("--out=") => {
                out = Some(arg.trim_start_matches("--out=").to_string());
            }
            other => return Err(format!("Argumen tidak dikenal: {other}")),
        }
    }

    out.filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "Argumen --out <path> wajib diisi.".to_string())
}
