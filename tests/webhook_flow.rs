use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use hmac::{Hmac, Mac};
use qris_api::{AppConfig, AppState, build_router, db::init_db};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

type HmacSha256 = Hmac<Sha256>;

const SYNTHETIC_STATIC: &str = "00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB";
const MASTER_KEY: &str = "test_master_key_1234567890_very_long_and_secret";
const DEK_B64: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

async fn setup_app() -> (axum::Router, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().expect("Buat tempdir");
    let db_path = temp_dir.path().join("test_webhook.db");
    let db_url = format!("sqlite://{}", db_path.to_str().unwrap());

    let pool = init_db(&db_url).await.expect("Init db");

    let config = AppConfig {
        app_env: "test".to_string(),
        app_bind: "127.0.0.1:0".to_string(),
        app_base_url: "http://127.0.0.1:0".to_string(),
        database_url: db_url,
        master_api_key: MASTER_KEY.to_string(),
        master_api_key_previous: None,
        data_encryption_key: DEK_B64.to_string(),
        require_signed_requests: false,
        allow_public_stateless: false,
    };

    let state = AppState::new(config, pool);
    (build_router(state), temp_dir)
}

#[tokio::test]
async fn test_full_webhook_auto_detect_flow() {
    let (app, _dir) = setup_app().await;

    // 1. Daftarkan Merchant
    let create_mch_req = serde_json::json!({
        "name": "Kos Auto Detect Test",
        "static_payload": SYNTHETIC_STATIC,
        "default_unique_code": true,
        "default_ttl_seconds": 900
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {}", MASTER_KEY))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(create_mch_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let mch_bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let mch_val: Value = serde_json::from_slice(&mch_bytes).unwrap();
    let merchant_id = mch_val["merchant"]["id"].as_str().unwrap().to_string();
    let merchant_key = mch_val["credentials"]["api_key"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Daftarkan Sumber Notifikasi Android Forwarder (Master Only)
    let create_src_req = serde_json::json!({
        "channel": "ANDROID",
        "label": "HP Kasir Kos"
    });

    let res_src = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{}/sources", merchant_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", MASTER_KEY))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(create_src_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let src_bytes = axum::body::to_bytes(res_src.into_body(), usize::MAX)
        .await
        .unwrap();
    let src_val: Value = serde_json::from_slice(&src_bytes).unwrap();
    let source_id = src_val["id"].as_str().unwrap().to_string();
    let source_secret = src_val["source_secret"].as_str().unwrap().to_string();

    // 3. Buat Transaksi Dinamis Rp 500.000 dengan kode unik otomatis
    let create_txn_req = serde_json::json!({
        "base_amount": 500000,
        "reference_label": "INV-M3-001"
    });

    let res_txn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{}/transactions", merchant_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", merchant_key))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(create_txn_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let txn_bytes = axum::body::to_bytes(res_txn.into_body(), usize::MAX)
        .await
        .unwrap();
    let txn_val: Value = serde_json::from_slice(&txn_bytes).unwrap();
    let txn_id = txn_val["id"].as_str().unwrap().to_string();
    let final_amount = txn_val["final_amount"].as_i64().unwrap();
    assert_eq!(txn_val["status"], "PENDING");

    // 4. Simulasi Forwarder Notifikasi Android Masuk (Kirim Webhook HMAC)
    let notif_text = format!("Pembayaran QRIS Rp {} berhasil diterima.", final_amount);
    let webhook_body = serde_json::json!({
        "package": "com.gojek.merchant",
        "app_name": "GoPay Merchant",
        "title": "Pembayaran diterima",
        "text": notif_text,
        "posted_at": "2026-11-01T10:04:30Z"
    })
    .to_string();

    let timestamp_str = "1767225600";
    let event_id_str = "notif-uuid-12345";

    let mut body_hasher = Sha256::new();
    body_hasher.update(webhook_body.as_bytes());
    let body_hash_hex = format!("{:x}", body_hasher.finalize());

    let canonical = format!("{}\n{}\n{}", timestamp_str, event_id_str, body_hash_hex);
    let mut mac = HmacSha256::new_from_slice(source_secret.as_bytes()).unwrap();
    mac.update(canonical.as_bytes());
    let signature = format!("v1={:x}", mac.finalize().into_bytes());

    let res_webhook = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/webhooks/mutation/{}", source_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Qris-Timestamp", timestamp_str)
                .header("X-Qris-Event-Id", event_id_str)
                .header("X-Qris-Signature", signature)
                .body(Body::from(webhook_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res_webhook.status(), StatusCode::ACCEPTED);
    let wh_bytes = axum::body::to_bytes(res_webhook.into_body(), usize::MAX)
        .await
        .unwrap();
    let wh_val: Value = serde_json::from_slice(&wh_bytes).unwrap();

    assert_eq!(wh_val["status"], "MATCHED");
    assert_eq!(wh_val["transaction_id"], txn_id);
    assert_eq!(wh_val["reason"], "exact_amount_match");

    // 5. Cek Status Transaksi: Status harus sudah otomatis berubah menjadi PAID!
    let res_check = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/transactions/{}/status", txn_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", merchant_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let check_bytes = axum::body::to_bytes(res_check.into_body(), usize::MAX)
        .await
        .unwrap();
    let check_val: Value = serde_json::from_slice(&check_bytes).unwrap();
    assert_eq!(check_val["status"], "PAID");
    assert!(check_val["paid_at"].is_i64());
}
