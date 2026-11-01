use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use qris_api::db::{backup, connect_file};
use qris_api::services::sweeper::SweeperWorker;
use qris_api::{AppConfig, AppState, build_router, db::init_db};
use tower::ServiceExt;

const MASTER_KEY: &str = "master_key_ops_tests_1234567890_abcdefghij";
const DEK_B64: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const SYNTHETIC_STATIC: &str = "00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB";

async fn setup(
    per_minute: u32,
    burst: u32,
    origins: Vec<String>,
) -> (Router, AppState, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().expect("Buat tempdir");
    let db_path = temp_dir.path().join("test_ops.db");
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
        rate_limit_per_minute: per_minute,
        rate_limit_burst: burst,
        rate_limit_image_per_minute: per_minute,
        cors_allowed_origins: origins,
        trust_proxy_headers: false,
    };

    let state = AppState::new(config, pool);
    (build_router(state.clone()), state, temp_dir)
}

/// Menyisipkan `ConnectInfo` layaknya server TCP sungguhan, sehingga
/// rate limit per IP dapat diuji tanpa bergantung pada header yang bisa dipalsukan.
fn with_client_ip(mut req: Request<Body>, ip: [u8; 4]) -> Request<Body> {
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            ip, 40000,
        ))));
    req
}

#[tokio::test]
async fn test_request_id_header_and_error_envelope() {
    let (app, _state, _dir) = setup(1000, 1000, vec![]).await;

    // 1. Respons sukses selalu memuat X-Request-Id.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let generated = res
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("X-Request-Id wajib ada")
        .to_string();
    assert!(generated.starts_with("req_"), "format ID: {generated}");

    // 2. ID valid dari klien dipakai kembali untuk korelasi lintas layanan.
    let res2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .header("x-request-id", "trace-abc-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res2.headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("trace-abc-123")
    );

    // 3. ID tidak aman diganti, bukan diteruskan mentah.
    let res3 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .header("x-request-id", "bad id with spaces")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(
        res3.headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("bad id with spaces")
    );

    // 4. Error envelope memuat request_id yang sama dengan header.
    let res4 = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/merchants")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res4.status(), StatusCode::UNAUTHORIZED);
    let header_id = res4
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("X-Request-Id pada error")
        .to_string();
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res4.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"]["request_id"], header_id);
}

#[tokio::test]
async fn test_security_headers_present() {
    let (app, _state, _dir) = setup(1000, 1000, vec![]).await;

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        res.headers()
            .get("x-frame-options")
            .and_then(|v| v.to_str().ok()),
        Some("DENY")
    );
    assert_eq!(
        res.headers()
            .get("referrer-policy")
            .and_then(|v| v.to_str().ok()),
        Some("no-referrer")
    );
}

#[tokio::test]
async fn test_rate_limit_returns_429_with_retry_after() {
    // burst = 1 → permintaan kedua harus ditolak.
    let (app, _state, _dir) = setup(1, 1, vec![]).await;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Health check dikecualikan dari rate limit, jadi gunakan endpoint API.
    assert_eq!(first.status(), StatusCode::OK);

    let api_req = |_: ()| {
        Request::builder()
            .method("GET")
            .uri("/api/v1/merchants")
            .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
            .body(Body::empty())
            .unwrap()
    };

    let r1 = app
        .clone()
        .oneshot(with_client_ip(api_req(()), [203, 0, 113, 10]))
        .await
        .unwrap();
    assert_eq!(
        r1.status(),
        StatusCode::OK,
        "permintaan pertama harus lolos"
    );

    let r2 = app
        .clone()
        .oneshot(with_client_ip(api_req(()), [203, 0, 113, 10]))
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        r2.headers().get(header::RETRY_AFTER).is_some(),
        "429 wajib menyertakan Retry-After"
    );

    let bytes = axum::body::to_bytes(r2.into_body(), usize::MAX)
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["error"]["code"], "RATE_LIMITED");
}

#[tokio::test]
async fn test_rate_limit_is_per_credential() {
    let (app, _state, _dir) = setup(1, 1, vec![]).await;

    let req = |token: &str| {
        Request::builder()
            .method("GET")
            .uri("/api/v1/merchants")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };

    // Kredensial A menghabiskan kuotanya (klien di IP A).
    let _ = app
        .clone()
        .oneshot(with_client_ip(req(MASTER_KEY), [203, 0, 113, 11]))
        .await
        .unwrap();
    let a2 = app
        .clone()
        .oneshot(with_client_ip(req(MASTER_KEY), [203, 0, 113, 11]))
        .await
        .unwrap();
    assert_eq!(a2.status(), StatusCode::TOO_MANY_REQUESTS);

    // Kredensial B dari IP lain masih punya kuota (bukan 429).
    let b1 = app
        .clone()
        .oneshot(with_client_ip(
            req("token_lain_yang_berbeda"),
            [203, 0, 113, 12],
        ))
        .await
        .unwrap();
    assert_ne!(b1.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_forwarded_for_cannot_bypass_ip_limit() {
    // `TRUST_PROXY_HEADERS=false` (default): X-Forwarded-For harus diabaikan.
    let (app, _state, _dir) = setup(1, 1, vec![]).await;

    let req = |token: &str, xff: &str| {
        Request::builder()
            .method("GET")
            .uri("/api/v1/merchants")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header("x-forwarded-for", xff)
            .body(Body::empty())
            .unwrap()
    };

    // Kredensial berbeda + XFF berbeda, tetapi tanpa ConnectInfo keduanya
    // jatuh ke bucket `ip:unknown`, sehingga permintaan kedua tetap ditolak.
    let r1 = app
        .clone()
        .oneshot(req(MASTER_KEY, "1.2.3.4"))
        .await
        .unwrap();
    assert_ne!(r1.status(), StatusCode::TOO_MANY_REQUESTS);

    let r2 = app
        .clone()
        .oneshot(req("token_kedua_yang_berbeda", "5.6.7.8"))
        .await
        .unwrap();
    assert_eq!(
        r2.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "X-Forwarded-For tidak boleh dipakai untuk melewati limit IP"
    );
}

#[tokio::test]
async fn test_cors_denied_by_default_and_allowed_when_configured() {
    // 1. Tanpa allowlist → tidak ada header CORS.
    let (app, _state, _dir) = setup(1000, 1000, vec![]).await;
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .header(header::ORIGIN, "https://evil.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        res.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "origin tak terdaftar tidak boleh mendapat header CORS"
    );

    // 2. Dengan allowlist → header CORS muncul untuk origin tersebut.
    let (app2, _state2, _dir2) =
        setup(1000, 1000, vec!["https://app.example.com".to_string()]).await;
    let res2 = app2
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .header(header::ORIGIN, "https://app.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res2.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok()),
        Some("https://app.example.com")
    );
}

#[tokio::test]
async fn test_transaction_response_is_no_store() {
    let (app, _state, _dir) = setup(1000, 1000, vec![]).await;

    // Buat merchant + transaksi.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "Merchant Cache",
                        "static_payload": SYNTHETIC_STATIC
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let val: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let merchant_id = val["merchant"]["id"].as_str().unwrap().to_string();
    let key = val["credentials"]["api_key"].as_str().unwrap().to_string();

    let res_txn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{merchant_id}/transactions"))
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"base_amount":50000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res_txn
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "respons pembuatan transaksi wajib no-store"
    );
    let txn: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res_txn.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let txn_id = txn["id"].as_str().unwrap().to_string();

    let res_get = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/transactions/{txn_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res_get.status(), StatusCode::OK);
    assert_eq!(
        res_get
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
}

#[tokio::test]
async fn test_qris_parse_rejects_oversized_json_and_bad_content_type() {
    let (app, _state, _dir) = setup(1000, 1000, vec![]).await;

    // 1. Body JSON > 256 KB → 413 (ditolak dari Content-Length sebelum dibaca).
    let huge = "a".repeat(300 * 1024);
    let body = format!(r#"{{"payload":"{huge}"}}"#);
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/qris/parse")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_LENGTH, body.len())
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // 2. Content-Type tidak didukung → 415.
    let res2 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/qris/parse")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("halo"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn test_test_webhook_flow() {
    let (app, state, _dir) = setup(1000, 1000, vec![]).await;

    // Buat merchant dengan callback_url
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "Webhook Test Merchant",
                        "static_payload": SYNTHETIC_STATIC,
                        "callback_url": "https://webhook.example.com/callback"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let val: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let merchant_id = val["merchant"]["id"].as_str().unwrap().to_string();

    // Trigger test-webhook
    let res_test = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{merchant_id}/test-webhook"))
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_test.status(), StatusCode::ACCEPTED);
    let dlv_res: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res_test.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(dlv_res["event_type"], "merchant.test");
    assert_eq!(dlv_res["status"], "PENDING");

    // Pastikan masuk ke tabel webhook_deliveries
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM webhook_deliveries WHERE merchant_id = ? AND event_type = 'merchant.test'"
    )
    .bind(&merchant_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_mark_paid_flow_and_audit() {
    let (app, state, _dir) = setup(1000, 1000, vec![]).await;

    // Buat merchant
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "Audit Test Merchant",
                        "static_payload": SYNTHETIC_STATIC
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let val: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let merchant_id = val["merchant"]["id"].as_str().unwrap().to_string();
    let key = val["credentials"]["api_key"].as_str().unwrap().to_string();

    // Buat transaksi
    let res_txn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/merchants/{merchant_id}/transactions"))
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"base_amount":25000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_txn.status(), StatusCode::CREATED);
    let txn: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res_txn.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let txn_id = txn["id"].as_str().unwrap().to_string();

    // 1. Mark paid gagal bila bukan master
    let res_forbidden = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/transactions/{txn_id}/mark-paid"))
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"reason":"Verifikasi manual"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_forbidden.status(), StatusCode::FORBIDDEN);

    // 2. Mark paid gagal jika alasan < 3 karakter
    let res_bad_reason = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/transactions/{txn_id}/mark-paid"))
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"reason":"ok"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_bad_reason.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // 3. Mark paid sukses dengan alasan valid oleh master
    let res_success = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/transactions/{txn_id}/mark-paid"))
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"reason":"Bukti transfer mutasi rekening bank valid"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_success.status(), StatusCode::OK);
    let paid_txn: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res_success.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(paid_txn["status"], "PAID");
    assert_eq!(paid_txn["payment_source"], "MANUAL");

    // 4. Mark paid ulang harus 409 TXN_ALREADY_PAID (TX-07)
    let res_dup = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/transactions/{txn_id}/mark-paid"))
                .header(header::AUTHORIZATION, format!("Bearer {MASTER_KEY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"reason":"Coba mark ulang"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res_dup.status(), StatusCode::CONFLICT);

    // 5. Verifikasi pencatatan audit log
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE action = 'transaction.mark_paid' AND resource_id = ?"
    )
    .bind(&txn_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(audit_count, 1, "audit log mark-paid harus tersimpan");
}

#[tokio::test]
async fn test_backup_is_consistent_and_never_overwrites() {
    let (_app, state, dir) = setup(1000, 1000, vec![]).await;
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        r#"INSERT INTO merchants (id, name, static_payload, static_payload_sha256,
           merchant_name, merchant_city, mcc, created_at, updated_at)
           VALUES ('mch_backup','B','P','h','B','J','5812',?,?)"#,
    )
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await
    .unwrap();

    let out = dir.path().join("backup").join("qris-backup.db");
    backup::backup_database(&state.config.database_url, &out)
        .await
        .expect("backup harus berhasil");
    assert!(out.exists(), "file backup harus dibuat");

    // Isi backup harus memuat data yang sama.
    let pool = connect_file(&out, true).await.expect("buka backup");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM merchants")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(count, 1);

    let integrity = backup::integrity_check_file(&out)
        .await
        .expect("integrity check backup");
    assert_eq!(integrity, "ok");

    // Backup tidak boleh menimpa file yang sudah ada.
    let err = backup::backup_database(&state.config.database_url, &out)
        .await
        .expect_err("backup kedua harus ditolak");
    assert!(err.contains("sudah ada"), "pesan error: {err}");
}

#[tokio::test]
async fn test_retention_deletes_only_old_rows() {
    let (_app, state, _dir) = setup(1000, 1000, vec![]).await;
    let now = chrono::Utc::now().timestamp_millis();
    let day = 24 * 60 * 60 * 1000i64;

    // Data induk (FK).
    sqlx::query(
        r#"INSERT INTO merchants (id, name, static_payload, static_payload_sha256,
           merchant_name, merchant_city, mcc, created_at, updated_at)
           VALUES ('mch_t','M','P','h','M','J','5812',?,?)"#,
    )
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await
    .unwrap();

    sqlx::query(
        r#"INSERT INTO notification_sources (id, merchant_id, channel, label,
           secret_prefix, secret_enc, created_at)
           VALUES ('src_t','mch_t','ANDROID','L','pfx','enc',?)"#,
    )
    .bind(now)
    .execute(&state.db)
    .await
    .unwrap();

    // Event lama (100 hari) dan baru.
    for (id, age_days) in [("evt_lama", 100i64), ("evt_baru", 1i64)] {
        sqlx::query(
            r#"INSERT INTO webhook_events (id, source_id, merchant_id, channel,
               event_key, raw_payload_sha256, received_at)
               VALUES (?, 'src_t', 'mch_t', 'ANDROID', ?, 'h', ?)"#,
        )
        .bind(id)
        .bind(id)
        .bind(now - age_days * day)
        .execute(&state.db)
        .await
        .unwrap();
    }

    // Delivery lama (DELIVERED) dan baru (PENDING).
    for (id, age_days, status) in [
        ("dlv_lama", 100i64, "DELIVERED"),
        ("dlv_baru", 1i64, "PENDING"),
    ] {
        sqlx::query(
            r#"INSERT INTO webhook_deliveries (id, merchant_id, event_type, url,
               payload_json, status, next_attempt_at, created_at)
               VALUES (?, 'mch_t', 'merchant.test', 'https://x.example.com', '{}', ?, ?, ?)"#,
        )
        .bind(id)
        .bind(status)
        .bind(now)
        .bind(now - age_days * day)
        .execute(&state.db)
        .await
        .unwrap();
    }

    let worker = SweeperWorker::new(state.db.clone(), 3600, 300);
    let (events, deliveries) = worker.run_retention().await.expect("Retensi");
    assert_eq!(events, 1, "hanya event lama yang dihapus");
    assert_eq!(deliveries, 1, "hanya delivery lama yang dihapus");

    let remaining_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_events")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(remaining_events, 1);

    let remaining_deliveries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM webhook_deliveries")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(remaining_deliveries, 1);

    // Transaksi/audit TIDAK boleh tersentuh worker retensi (AGENTS §12.11).
    let txns: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(txns, 0);
}
