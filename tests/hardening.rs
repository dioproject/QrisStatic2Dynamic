use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode, header},
    routing::post,
};
use hmac::{Hmac, Mac};
use qris_api::services::outbound::OutboundWorker;
use qris_api::{AppConfig, AppState, build_router, db::init_db};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

type HmacSha256 = Hmac<Sha256>;

const MASTER_KEY: &str = "master_key_hardening_tests_1234567890_abcdef";
const DEK_B64: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const SYNTHETIC_STATIC: &str = "00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB";

async fn setup(
    require_signed: bool,
    allow_insecure: bool,
) -> (Router, AppState, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().expect("Buat tempdir");
    let db_path = temp_dir.path().join("test_hardening.db");
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
        require_signed_requests: require_signed,
        allow_public_stateless: false,
        allow_insecure_callbacks: allow_insecure,
        rate_limit_per_minute: 1000,
        rate_limit_burst: 1000,
        rate_limit_image_per_minute: 1000,
        cors_allowed_origins: vec![],
        trust_proxy_headers: false,
    };

    let state = AppState::new(config, pool);
    (build_router(state.clone()), state, temp_dir)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn signing_headers(secret: &str, method: &str, path: &str, body: &str) -> Vec<(String, String)> {
    let ts = chrono::Utc::now().timestamp().to_string();
    let nonce = format!("nonce{}", uuid_like());
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}",
        ts,
        nonce,
        method,
        path,
        sha256_hex(body.as_bytes())
    );

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(canonical.as_bytes());
    let sig = format!("v1={:x}", mac.finalize().into_bytes());

    vec![
        ("X-Qris-Timestamp".to_string(), ts),
        ("X-Qris-Nonce".to_string(), nonce),
        ("X-Qris-Signature".to_string(), sig),
    ]
}

/// Nonce unik tanpa menambah dependency: gabungan waktu + alamat stack.
fn uuid_like() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", now)
}

fn merchant_body(static_payload: &str, callback_url: Option<&str>) -> String {
    serde_json::json!({
        "name": "Merchant Hardening",
        "static_payload": static_payload,
        "default_unique_code": true,
        "default_ttl_seconds": 900,
        "callback_url": callback_url
    })
    .to_string()
}

#[tokio::test]
async fn test_signed_middleware_rejects_unsigned_post() {
    let (app, _state, _dir) = setup(true, true).await;
    let body = merchant_body(SYNTHETIC_STATIC, None);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["error"]["code"], "SIGNATURE_REQUIRED");
}

#[tokio::test]
async fn test_signed_middleware_accepts_valid_signature() {
    let (app, _state, _dir) = setup(true, true).await;
    let body = merchant_body(SYNTHETIC_STATIC, None);
    let headers = signing_headers(MASTER_KEY, "POST", "/api/v1/merchants", &body);

    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/merchants")
        .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
        .header(header::CONTENT_TYPE, "application/json");

    for (k, v) in &headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    let res = app
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_signed_middleware_rejects_bad_signature() {
    let (app, _state, _dir) = setup(true, true).await;
    let body = merchant_body(SYNTHETIC_STATIC, None);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    "X-Qris-Timestamp",
                    chrono::Utc::now().timestamp().to_string(),
                )
                .header("X-Qris-Nonce", "nonce0000000000000001")
                .header("X-Qris-Signature", "v1=deadbeef")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["error"]["code"], "SIGNATURE_INVALID");
}

#[tokio::test]
async fn test_signed_middleware_skips_inbound_webhook() {
    // Mode strict aktif. Inbound webhook memakai kanonik HMAC sendiri (WH-04)
    // dan TIDAK memakai X-Qris-Nonce, jadi middleware harus melewatinya.
    let (app, _state, _dir) = setup(true, true).await;

    let body = r#"{"text":"Pembayaran QRIS Rp 1000 diterima"}"#;
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/webhooks/mutation/src_tidak_ada")
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    "X-Qris-Timestamp",
                    chrono::Utc::now().timestamp().to_string(),
                )
                .header("X-Qris-Event-Id", "evt-1")
                .header("X-Qris-Signature", "v1=deadbeef")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    // Sampai ke handler webhook (source tidak ada) membuktikan middleware tidak
    // memblokir dengan SIGNATURE_REQUIRED karena nonce tidak dikirim.
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["error"]["code"], "SOURCE_NOT_FOUND");
}

// ---------- Outbound worker ----------

#[derive(Clone)]
struct ReceiverState {
    captured: Arc<Mutex<Vec<CapturedDelivery>>>,
}

struct CapturedDelivery {
    headers: HeaderMap,
    body: String,
}

async fn capture_handler(
    State(st): State<ReceiverState>,
    headers: HeaderMap,
    body: String,
) -> StatusCode {
    st.captured
        .lock()
        .unwrap()
        .push(CapturedDelivery { headers, body });
    StatusCode::OK
}

async fn start_receiver() -> (String, Arc<Mutex<Vec<CapturedDelivery>>>) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/hook", post(capture_handler))
        .with_state(ReceiverState {
            captured: captured.clone(),
        });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{}/hook", addr), captured)
}

#[tokio::test]
async fn test_outbound_worker_delivers_signed_callback() {
    let (callback_url, captured) = start_receiver().await;
    // allow_insecure = true supaya receiver lokal (http) diizinkan pada test.
    let (app, state, _dir) = setup(false, true).await;

    // 1. Merchant dengan callback_url lokal.
    let body = merchant_body(SYNTHETIC_STATIC, Some(&callback_url));
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let val: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let merchant_id = val["merchant"]["id"].as_str().unwrap().to_string();
    let merchant_key = val["credentials"]["api_key"].as_str().unwrap().to_string();
    let signing_secret = val["credentials"]["signing_secret"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Source Android.
    let res_src = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{merchant_id}/sources"))
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"channel":"ANDROID","label":"HP Kasir"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_src.status(), StatusCode::CREATED);

    let src_val: Value = serde_json::from_slice(
        &axum::body::to_bytes(res_src.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let source_id = src_val["id"].as_str().unwrap().to_string();
    let source_secret = src_val["source_secret"].as_str().unwrap().to_string();

    // 3. Transaksi dinamis.
    let res_txn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{merchant_id}/transactions"))
                .header(header::AUTHORIZATION, format!("Bearer {merchant_key}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"base_amount":500000,"reference_label":"INV-OW-1"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_txn.status(), StatusCode::CREATED);

    let txn_val: Value = serde_json::from_slice(
        &axum::body::to_bytes(res_txn.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let txn_id = txn_val["id"].as_str().unwrap().to_string();
    let final_amount = txn_val["final_amount"].as_i64().unwrap();

    // 4. Webhook mutasi masuk → matcher membuat baris outbox.
    let notif = format!("Pembayaran QRIS Rp {final_amount} berhasil diterima.");
    let wh_body = serde_json::json!({ "package": "com.gojek.merchant", "text": notif }).to_string();
    let ts = chrono::Utc::now().timestamp().to_string();
    let evt = "notif-ow-1";
    let canonical = format!("{ts}\n{evt}\n{}", sha256_hex(wh_body.as_bytes()));
    let mut mac = HmacSha256::new_from_slice(source_secret.as_bytes()).unwrap();
    mac.update(canonical.as_bytes());
    let wh_sig = format!("v1={:x}", mac.finalize().into_bytes());

    let res_wh = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/webhooks/mutation/{source_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Qris-Timestamp", ts)
                .header("X-Qris-Event-Id", evt)
                .header("X-Qris-Signature", wh_sig)
                .body(Body::from(wh_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_wh.status(), StatusCode::ACCEPTED);

    let wh_val: Value = serde_json::from_slice(
        &axum::body::to_bytes(res_wh.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(wh_val["status"], "MATCHED");

    // 5. Jalankan worker outbound sekali.
    let worker = OutboundWorker::new(state.db.clone(), DEK_B64.to_string(), 5, true);
    let processed = worker.process_batch().await.expect("Worker batch");
    assert_eq!(processed, 1, "Harus ada tepat satu delivery diproses");

    // 6. Verifikasi callback diterima + signature valid (OW-03).
    //    Data disalin keluar guard agar tidak menahan lock melewati await.
    let (got_ts, got_evt, got_sig, got_body) = {
        let deliveries = captured.lock().unwrap();
        assert_eq!(deliveries.len(), 1, "Merchant harus menerima satu callback");
        let got = &deliveries[0];
        (
            got.headers
                .get("X-Qris-Timestamp")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
            got.headers
                .get("X-Qris-Event-Id")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
            got.headers
                .get("X-Qris-Signature")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
            got.body.clone(),
        )
    };

    let expected_canonical = format!("{got_ts}\n{got_evt}\n{}", sha256_hex(got_body.as_bytes()));
    let mut mac2 = HmacSha256::new_from_slice(signing_secret.as_bytes()).unwrap();
    mac2.update(expected_canonical.as_bytes());
    let expected_sig = format!("v1={:x}", mac2.finalize().into_bytes());
    assert_eq!(got_sig, expected_sig, "Signature callback harus valid");

    // 7. Payload sesuai kontrak & tidak memuat secret (OW-10).
    let payload: Value = serde_json::from_str(&got_body).unwrap();
    assert_eq!(payload["event"], "transaction.paid");
    assert_eq!(payload["data"]["transaction_id"], txn_id);
    assert_eq!(payload["data"]["amount"]["final"], final_amount);
    assert_eq!(payload["data"]["status"], "PAID");
    assert!(!got_body.contains("qss_live_"));
    assert!(!got_body.contains("qsk_live_"));
    assert!(!got_body.contains("secret"));

    // 8. Status delivery harus DELIVERED.
    let status: String =
        sqlx::query_scalar("SELECT status FROM webhook_deliveries WHERE transaction_id = ?")
            .bind(&txn_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(status, "DELIVERED");
}

// ---------- SSE ----------

#[tokio::test]
async fn test_sse_emits_terminal_status_and_closes() {
    let (app, _state, _dir) = setup(false, true).await;

    // 1. Merchant + transaksi.
    let body = merchant_body(SYNTHETIC_STATIC, None);
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let merchant_id = val["merchant"]["id"].as_str().unwrap().to_string();
    let merchant_key = val["credentials"]["api_key"].as_str().unwrap().to_string();

    let res_txn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{merchant_id}/transactions"))
                .header(header::AUTHORIZATION, format!("Bearer {merchant_key}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"base_amount":150000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let txn_val: Value = serde_json::from_slice(
        &axum::body::to_bytes(res_txn.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let txn_id = txn_val["id"].as_str().unwrap().to_string();

    // 2. Batalkan agar status terminal → stream menutup diri.
    let res_cancel = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/transactions/{txn_id}/cancel"))
                .header(header::AUTHORIZATION, format!("Bearer {merchant_key}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"reason":"uji SSE"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_cancel.status(), StatusCode::OK);

    // 3. Buka stream dan baca sampai selesai.
    let res_stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/transactions/{txn_id}/stream"))
                .header(header::AUTHORIZATION, format!("Bearer {merchant_key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res_stream.status(), StatusCode::OK);
    assert_eq!(
        res_stream
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
        "text/event-stream"
    );

    let body = axum::body::to_bytes(res_stream.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);

    assert!(
        text.contains("event: status"),
        "harus ada event status: {text}"
    );
    assert!(
        text.contains("\"status\":\"CANCELED\""),
        "harus memuat CANCELED: {text}"
    );
    assert!(text.contains(&txn_id), "harus memuat id transaksi");
    assert!(text.contains("retry: 3000"), "harus mengirim retry: 3000");
}

#[tokio::test]
async fn test_sse_rejects_other_merchant() {
    let (app, _state, _dir) = setup(false, true).await;

    // Merchant A + transaksi A.
    let res_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(merchant_body(SYNTHETIC_STATIC, None)))
                .unwrap(),
        )
        .await
        .unwrap();
    let val_a: Value = serde_json::from_slice(
        &axum::body::to_bytes(res_a.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let mch_a = val_a["merchant"]["id"].as_str().unwrap().to_string();
    let key_a = val_a["credentials"]["api_key"]
        .as_str()
        .unwrap()
        .to_string();

    // Merchant B: payload statis berbeda (nama + NMID) dengan CRC dihitung ulang.
    let base = &SYNTHETIC_STATIC[..SYNTHETIC_STATIC.len() - 8]; // buang "6304<CRC>"
    let variant = base
        .replace("KOS SAYA", "KOS LAIN")
        .replace("ID1024001234567", "ID1024007654321");
    let with_crc = format!("{variant}6304");
    let static_b = format!(
        "{}{}",
        with_crc,
        qris_api::domain::crc16::crc16_hex(with_crc.as_bytes())
    );

    let res_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(merchant_body(&static_b, None)))
                .unwrap(),
        )
        .await
        .unwrap();
    let val_b: Value = serde_json::from_slice(
        &axum::body::to_bytes(res_b.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let key_b = val_b["credentials"]["api_key"]
        .as_str()
        .unwrap()
        .to_string();

    let res_txn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{mch_a}/transactions"))
                .header(header::AUTHORIZATION, format!("Bearer {key_a}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"base_amount":99000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let txn_val: Value = serde_json::from_slice(
        &axum::body::to_bytes(res_txn.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let txn_id = txn_val["id"].as_str().unwrap().to_string();

    // Merchant B tidak boleh membaca stream transaksi milik A (MT-01).
    let res_stream = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/transactions/{txn_id}/stream"))
                .header(header::AUTHORIZATION, format!("Bearer {key_b}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res_stream.status(), StatusCode::FORBIDDEN);
}
