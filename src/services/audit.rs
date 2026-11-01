use crate::auth::Principal;
use crate::db::DbPool;
use crate::error::AppError;

pub struct AuditLogger;

impl AuditLogger {
    /// Mencatat log audit terstruktur ke tabel `audit_logs` (ERP §3.9, RULES §4 AU).
    pub async fn log(
        pool: &DbPool,
        principal: &Principal,
        action: &str,
        resource_type: &str,
        resource_id: &str,
        merchant_id: Option<&str>,
        changes: Option<serde_json::Value>,
    ) -> Result<(), AppError> {
        let id = format!("aud_{}", ulid::Ulid::new());
        let (actor_type, actor_id) = match principal {
            Principal::Master => ("MASTER", "master"),
            Principal::Merchant { merchant_id, .. } => ("MERCHANT", merchant_id.as_str()),
        };

        let now = chrono::Utc::now().timestamp_millis();
        let changes_str = changes.map(|c| c.to_string());

        sqlx::query(
            r#"
            INSERT INTO audit_logs (
                id, merchant_id, actor_type, actor_id, action,
                resource_type, resource_id, changes_json, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(merchant_id)
        .bind(actor_type)
        .bind(actor_id)
        .bind(action)
        .bind(resource_type)
        .bind(resource_id)
        .bind(changes_str)
        .bind(now)
        .execute(pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(())
    }
}
