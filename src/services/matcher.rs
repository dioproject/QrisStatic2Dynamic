use crate::db::DbPool;
use crate::error::AppError;
use serde::Serialize;
use ulid::Ulid;

#[derive(Debug, Clone, Serialize)]
pub struct MatchResult {
    pub event_id: String,
    pub status: &'static str,
    pub transaction_id: Option<String>,
    pub reason: &'static str,
}

#[derive(sqlx::FromRow)]
struct MatchCandidate {
    id: String,
    reference_label: Option<String>,
    base_amount: i64,
    fee_amount: i64,
    unique_code: i64,
    final_amount: i64,
}

pub struct MatchEventParams<'a> {
    pub source_id: &'a str,
    pub merchant_id: &'a str,
    pub channel: &'a str,
    pub event_key: &'a str,
    pub raw_payload_sha256: &'a str,
    pub raw_text_masked: Option<&'a str>,
    pub parsed_amount: Option<i64>,
    pub parsed_direction: &'a str,
    pub parser_version: &'a str,
    pub match_reason: &'a str,
}

pub struct MatcherService<'a> {
    pool: &'a DbPool,
}

impl<'a> MatcherService<'a> {
    pub fn new(pool: &'a DbPool) -> Self {
        Self { pool }
    }

    /// Mencocokkan pembayaran nominal mutasi dengan transaksi PENDING milik merchant (RULES §11)
    pub async fn match_payment(
        &self,
        params: MatchEventParams<'_>,
    ) -> Result<MatchResult, AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        // 1. Cek Duplikasi Event (RULES §9.1 WH-10)
        let dup = sqlx::query_scalar::<_, String>(
            "SELECT id FROM webhook_events WHERE source_id = ? AND event_key = ?",
        )
        .bind(params.source_id)
        .bind(params.event_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if let Some(existing_event_id) = dup {
            tx.commit()
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;
            return Ok(MatchResult {
                event_id: existing_event_id,
                status: "DUPLICATE",
                transaction_id: None,
                reason: "duplicate_event_key",
            });
        }

        let event_id = format!("evt_{}", Ulid::new());
        let now = chrono::Utc::now().timestamp_millis();

        // 2. Jika arah keluar (DR) atau tidak ada nominal -> IGNORED (WH-14, WH-15)
        if params.parsed_direction != "CR" || params.parsed_amount.is_none() {
            sqlx::query(
                r#"
                INSERT INTO webhook_events (
                    id, source_id, merchant_id, channel, event_key,
                    raw_payload_sha256, raw_text_masked, parsed_amount, parsed_direction,
                    parser_version, match_reason, status, received_at, processed_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'IGNORED', ?, ?)
                "#,
            )
            .bind(&event_id)
            .bind(params.source_id)
            .bind(params.merchant_id)
            .bind(params.channel)
            .bind(params.event_key)
            .bind(params.raw_payload_sha256)
            .bind(params.raw_text_masked)
            .bind(params.parsed_amount)
            .bind(params.parsed_direction)
            .bind(params.parser_version)
            .bind(params.match_reason)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

            tx.commit()
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            return Ok(MatchResult {
                event_id,
                status: "IGNORED",
                transaction_id: None,
                reason: if params.parsed_direction != "CR" {
                    "direction_not_credit"
                } else {
                    "amount_missing"
                },
            });
        }

        let amount = params.parsed_amount.unwrap();
        // Syarat waktu: expires_at + grace > now (grace = 300 detik = 300.000 ms)
        let grace_ms = 300_000i64;

        // 3. Cari Transaksi Kandidat (MT-01 s/d MT-04)
        let candidate = sqlx::query_as::<_, MatchCandidate>(
            r#"
            SELECT id, reference_label, base_amount, fee_amount, unique_code, final_amount
            FROM transactions
            WHERE merchant_id = ?
              AND status = 'PENDING'
              AND final_amount = ?
              AND (expires_at + ?) >= ?
            LIMIT 1
            "#,
        )
        .bind(params.merchant_id)
        .bind(amount)
        .bind(grace_ms)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if let Some(candidate) = candidate {
            let txn_id = candidate.id.clone();
            // Transisi atomik status transaksi menjadi PAID (MT-05)
            let rows_affected = sqlx::query(
                r#"
                UPDATE transactions
                SET status = 'PAID', paid_at = ?, payment_source = ?, paid_event_id = ?, updated_at = ?
                WHERE id = ? AND status = 'PENDING'
                "#,
            )
            .bind(now)
            .bind(params.channel)
            .bind(&event_id)
            .bind(now)
            .bind(&txn_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .rows_affected();

            if rows_affected > 0 {
                // Catat Webhook Event sebagai MATCHED
                sqlx::query(
                    r#"
                    INSERT INTO webhook_events (
                        id, source_id, merchant_id, channel, event_key,
                        raw_payload_sha256, raw_text_masked, parsed_amount, parsed_direction,
                        parser_version, match_reason, status, transaction_id, received_at, processed_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'MATCHED', ?, ?, ?)
                    "#,
                )
                .bind(&event_id)
                .bind(params.source_id)
                .bind(params.merchant_id)
                .bind(params.channel)
                .bind(params.event_key)
                .bind(params.raw_payload_sha256)
                .bind(params.raw_text_masked)
                .bind(amount)
                .bind(params.parsed_direction)
                .bind(params.parser_version)
                .bind(params.match_reason)
                .bind(&txn_id)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

                // Buat Outbox Delivery jika merchant punya callback_url (OW-01 s/d OW-03)
                let merchant_cb = sqlx::query_scalar::<_, Option<String>>(
                    "SELECT callback_url FROM merchants WHERE id = ?",
                )
                .bind(params.merchant_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

                if let Some(cb_url) = merchant_cb {
                    let dlv_id = format!("dlv_{}", Ulid::new());
                    // Payload snapshot mengikuti kontrak ERP §9.x (OW-08, OW-10).
                    let payload_json = crate::services::outbound::build_paid_payload(
                        crate::services::outbound::PaidPayloadInput {
                            event_id: &dlv_id,
                            transaction_id: &txn_id,
                            merchant_id: params.merchant_id,
                            reference_label: candidate.reference_label.as_deref(),
                            base: candidate.base_amount,
                            fee: candidate.fee_amount,
                            unique_code: candidate.unique_code,
                            final_amount: candidate.final_amount,
                            paid_at_ms: now,
                            payment_source: Some(params.channel),
                        },
                    )
                    .to_string();

                    sqlx::query(
                        r#"
                        INSERT INTO webhook_deliveries (
                            id, merchant_id, transaction_id, event_type,
                            url, payload_json, status, attempts, next_attempt_at, created_at
                        ) VALUES (?, ?, ?, 'transaction.paid', ?, ?, 'PENDING', 0, ?, ?)
                        "#,
                    )
                    .bind(&dlv_id)
                    .bind(params.merchant_id)
                    .bind(&txn_id)
                    .bind(cb_url)
                    .bind(payload_json)
                    .bind(now)
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| AppError::Internal(e.to_string()))?;
                }

                tx.commit()
                    .await
                    .map_err(|e| AppError::Internal(e.to_string()))?;

                return Ok(MatchResult {
                    event_id,
                    status: "MATCHED",
                    transaction_id: Some(txn_id),
                    reason: "exact_amount_match",
                });
            }
        }

        // 4. Jika tidak ada transaksi cocok -> UNMATCHED (MT-06)
        sqlx::query(
            r#"
            INSERT INTO webhook_events (
                id, source_id, merchant_id, channel, event_key,
                raw_payload_sha256, raw_text_masked, parsed_amount, parsed_direction,
                parser_version, match_reason, status, received_at, processed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'UNMATCHED', ?, ?)
            "#,
        )
        .bind(&event_id)
        .bind(params.source_id)
        .bind(params.merchant_id)
        .bind(params.channel)
        .bind(params.event_key)
        .bind(params.raw_payload_sha256)
        .bind(params.raw_text_masked)
        .bind(amount)
        .bind(params.parsed_direction)
        .bind(params.parser_version)
        .bind(params.match_reason)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(MatchResult {
            event_id,
            status: "UNMATCHED",
            transaction_id: None,
            reason: "no_matching_pending_transaction",
        })
    }
}
