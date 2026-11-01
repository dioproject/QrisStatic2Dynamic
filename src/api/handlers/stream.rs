use crate::auth::{Principal, RequireAuth};
use crate::error::AppError;
use crate::services::transaction_service::TransactionService;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::stream::{self, Stream};
use serde_json::json;
use std::convert::Infallible;
use std::time::{Duration, Instant};

/// Interval polling status di DB.
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Stream ditutup otomatis setelah 5 menit (ERP §9.6).
const MAX_STREAM_DURATION: Duration = Duration::from_secs(300);
/// Heartbeat komentar `: ping` setiap 15 detik (ERP §9.6).
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

struct StreamState {
    txn_id: String,
    pool: crate::db::DbPool,
    last_status: Option<String>,
    started: Instant,
    finished: bool,
}

struct StatusSnapshot {
    status: String,
    paid_at: Option<i64>,
    final_amount: i64,
}

fn is_terminal(status: &str) -> bool {
    matches!(status, "PAID" | "EXPIRED" | "CANCELED")
}

fn rfc3339_from_ms(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

async fn fetch_snapshot(pool: &crate::db::DbPool, txn_id: &str) -> Option<StatusSnapshot> {
    let row = sqlx::query_as::<_, (String, Option<i64>, i64)>(
        "SELECT status, paid_at, final_amount FROM transactions WHERE id = ?",
    )
    .bind(txn_id)
    .fetch_optional(pool)
    .await
    .ok()??;

    Some(StatusSnapshot {
        status: row.0,
        paid_at: row.1,
        final_amount: row.2,
    })
}

/// GET /api/v1/transactions/{id}/stream (Server-Sent Events)
pub async fn transaction_stream_handler(
    State(state): State<AppState>,
    RequireAuth(principal): RequireAuth,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    // 1. Pastikan transaksi ada dan milik principal (MT-01).
    let service = TransactionService::new(&state.db);
    let txn = service.get_transaction(&id).await?;

    if let Principal::Merchant { merchant_id, .. } = &principal
        && merchant_id != &txn.merchant_id
    {
        return Err(AppError::AuthForbidden);
    }

    // 2. Susun stream status.
    let initial = StreamState {
        txn_id: txn.id,
        pool: state.db.clone(),
        last_status: None,
        started: Instant::now(),
        finished: false,
    };

    let stream = stream::unfold(initial, |mut st| async move {
        loop {
            if st.finished || st.started.elapsed() >= MAX_STREAM_DURATION {
                return None;
            }

            let snapshot = match fetch_snapshot(&st.pool, &st.txn_id).await {
                Some(s) => s,
                None => return None,
            };

            let changed = st.last_status.as_deref() != Some(snapshot.status.as_str());
            let terminal = is_terminal(&snapshot.status);

            if changed || terminal {
                st.last_status = Some(snapshot.status.clone());
                // Tandai selesai supaya iterasi berikutnya menutup stream.
                if terminal {
                    st.finished = true;
                }

                let data = json!({
                    "id": st.txn_id,
                    "status": snapshot.status,
                    "paid_at": snapshot.paid_at.map(rfc3339_from_ms),
                    "amount": { "final": snapshot.final_amount }
                });

                let event = Event::default()
                    .event("status")
                    .id(st.txn_id.clone())
                    .retry(Duration::from_secs(3))
                    .data(data.to_string());

                return Some((Ok::<Event, Infallible>(event), st));
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });

    // 3. `retry: 3000` juga dikirim sebagai komentar awal agar client tahu jeda reconnect.
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(HEARTBEAT_INTERVAL).text("ping")))
}
