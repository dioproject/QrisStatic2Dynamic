use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use qris_api::{AppConfig, AppState, build_router, db::init_db};
use serde_json::Value;
use tower::ServiceExt;

const SYNTHETIC_STATIC: &str = "00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB";
const MASTER_KEY: &str = "test_master_key_1234567890_very_long_and_secret";
const DEK_B64: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="; // 32 bytes valid base64

async fn setup_app() -> (axum::Router, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().expect("Buat tempdir");
    let db_path = temp_dir.path().join("test.db");
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
        allow_insecure_callbacks: false,
    };

    let state = AppState::new(config, pool);
    (build_router(state), temp_dir)
}

#[tokio::test]
async fn test_merchant_lifecycle_and_transactions() {
    let (app, _dir) = setup_app().await;

    // 1. Daftarkan Merchant Baru (Wajib Master Key)
    let create_mch_req = serde_json::json!({
        "name": "Kos Test Sukses",
        "static_payload": SYNTHETIC_STATIC,
        "default_fee_mode": "FIXED",
        "default_fee_fixed": 1500,
        "default_unique_code": true,
        "default_ttl_seconds": 600,
        "callback_url": "https://example.com/webhook"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/merchants")
        .header(header::AUTHORIZATION, format!("Bearer {}", MASTER_KEY))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(create_mch_req.to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(&body_bytes).unwrap();

    let merchant_id = val["merchant"]["id"].as_str().unwrap().to_string();
    let api_key = val["credentials"]["api_key"].as_str().unwrap().to_string();
    assert!(api_key.starts_with("qsk_live_"));

    // 2. Coba Daftarkan Lagi (Harus ditolak: MERCHANT_NMID_CONFLICT / MERCHANT_ALREADY_REGISTERED)
    let req_dup = Request::builder()
        .method("POST")
        .uri("/api/v1/merchants")
        .header(header::AUTHORIZATION, format!("Bearer {}", MASTER_KEY))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(create_mch_req.to_string()))
        .unwrap();

    let res_dup = app.clone().oneshot(req_dup).await.unwrap();
    assert_eq!(res_dup.status(), StatusCode::CONFLICT);

    // 3. Buat Transaksi Dinamis memakai Merchant Key
    let create_txn_req = serde_json::json!({
        "base_amount": 500000,
        "reference_label": "INV-001",
        "merchant_ref": "order-123"
    });

    let req_txn = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/merchants/{}/transactions", merchant_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", api_key))
        .header(header::CONTENT_TYPE, "application/json")
        .header("Idempotency-Key", "idemp-001")
        .body(Body::from(create_txn_req.to_string()))
        .unwrap();

    let res_txn = app.clone().oneshot(req_txn).await.unwrap();
    assert_eq!(res_txn.status(), StatusCode::CREATED);

    let txn_bytes = axum::body::to_bytes(res_txn.into_body(), usize::MAX)
        .await
        .unwrap();
    let txn_val: Value = serde_json::from_slice(&txn_bytes).unwrap();

    let txn_id = txn_val["id"].as_str().unwrap().to_string();
    assert_eq!(txn_val["status"], "PENDING");
    assert_eq!(txn_val["base_amount"], 500000);
    assert_eq!(txn_val["fee_amount"], 1500);

    let u_code = txn_val["unique_code"].as_i64().unwrap();
    assert!((1..=999).contains(&u_code));
    let expected_final = 500000 + 1500 + u_code;
    assert_eq!(txn_val["final_amount"], expected_final);

    // 4. Uji Idempotency-Key: Request sama harus mengembalikan transaksi yang sama persis
    let req_idem = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/merchants/{}/transactions", merchant_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", api_key))
        .header(header::CONTENT_TYPE, "application/json")
        .header("Idempotency-Key", "idemp-001")
        .body(Body::from(create_txn_req.to_string()))
        .unwrap();

    let res_idem = app.clone().oneshot(req_idem).await.unwrap();
    assert_eq!(res_idem.status(), StatusCode::CREATED);
    let idem_bytes = axum::body::to_bytes(res_idem.into_body(), usize::MAX)
        .await
        .unwrap();
    let idem_val: Value = serde_json::from_slice(&idem_bytes).unwrap();
    assert_eq!(idem_val["id"], txn_id);

    // 5. Cek Status Transaksi
    let req_status = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/transactions/{}/status", txn_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", api_key))
        .body(Body::empty())
        .unwrap();

    let res_status = app.clone().oneshot(req_status).await.unwrap();
    assert_eq!(res_status.status(), StatusCode::OK);

    // 6. Cek Ambil QR PNG
    let req_png = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/transactions/{}/qr.png", txn_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", api_key))
        .body(Body::empty())
        .unwrap();

    let res_png = app.clone().oneshot(req_png).await.unwrap();
    assert_eq!(res_png.status(), StatusCode::OK);
    assert_eq!(
        res_png.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );

    // 7. Batalkan Transaksi (Cancel)
    let cancel_req = serde_json::json!({
        "reason": "Pembeli membatalkan pesanan"
    });

    let req_cancel = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/transactions/{}/cancel", txn_id))
        .header(header::AUTHORIZATION, format!("Bearer {}", api_key))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(cancel_req.to_string()))
        .unwrap();

    let res_cancel = app.clone().oneshot(req_cancel).await.unwrap();
    assert_eq!(res_cancel.status(), StatusCode::OK);
    let cancel_bytes = axum::body::to_bytes(res_cancel.into_body(), usize::MAX)
        .await
        .unwrap();
    let cancel_val: Value = serde_json::from_slice(&cancel_bytes).unwrap();
    assert_eq!(cancel_val["status"], "CANCELED");
}
