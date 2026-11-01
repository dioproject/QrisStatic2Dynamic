use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use qris_api::auth::Principal;
use qris_api::auth::hmac::SignedRequestValidator;
use qris_api::{AppConfig, AppState, build_router, db::init_db};
use tower::ServiceExt;

const MASTER_KEY: &str = "master_key_for_auth_tests_1234567890_very_long";
const DEK_B64: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

async fn setup_app() -> (axum::Router, tempfile::TempDir, AppState) {
    let temp_dir = tempfile::tempdir().expect("Buat tempdir");
    let db_path = temp_dir.path().join("test_auth.db");
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
        require_signed_requests: true, // Strict HMAC mode aktif
        allow_public_stateless: false,
    };

    let state = AppState::new(config, pool);
    (build_router(state.clone()), temp_dir, state)
}

#[tokio::test]
async fn test_master_auth_rejection() {
    let (app, _dir, _state) = setup_app().await;

    // Akses endpoint master tanpa Authorization header
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/merchants")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Akses endpoint master dengan key salah
    let res_bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/merchants")
                .header(header::AUTHORIZATION, "Bearer salah_total")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res_bad.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_signed_request_validator_nonce_replay() {
    let (_app, _dir, state) = setup_app().await;

    let principal = Principal::Master;
    let method = "POST";
    let path = "/api/v1/merchants";
    let body = b"{}";
    let ts = chrono::Utc::now().timestamp().to_string();
    let nonce = "nonce1234567890abcdef";

    // Hitung signature valid
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    type HmacSha256 = Hmac<Sha256>;

    let mut hasher = Sha256::new();
    hasher.update(body);
    let body_hash_hex = format!("{:x}", hasher.finalize());

    let canonical = format!("{}\n{}\n{}\n{}\n{}", ts, nonce, method, path, body_hash_hex);
    let mut mac = HmacSha256::new_from_slice(MASTER_KEY.as_bytes()).unwrap();
    mac.update(canonical.as_bytes());
    let sig = format!("v1={:x}", mac.finalize().into_bytes());

    // 1. Validasi pertama harus sukses
    let first = SignedRequestValidator::validate(
        &state,
        &principal,
        qris_api::auth::hmac::RequestSignatureParams {
            method,
            path_and_query: path,
            raw_body: body,
            timestamp_str: Some(&ts),
            nonce_str: Some(nonce),
            signature_str: Some(&sig),
        },
    )
    .await;

    assert!(first.is_ok(), "Request pertama harus lolos verifikasi");

    // 2. Replay request dengan nonce yang sama harus DITOLAK (SIGNATURE_REPLAYED)
    let replay = SignedRequestValidator::validate(
        &state,
        &principal,
        qris_api::auth::hmac::RequestSignatureParams {
            method,
            path_and_query: path,
            raw_body: body,
            timestamp_str: Some(&ts),
            nonce_str: Some(nonce),
            signature_str: Some(&sig),
        },
    )
    .await;

    assert!(
        matches!(replay, Err(qris_api::error::AppError::SignatureReplayed)),
        "Replay attack dengan nonce sama wajib ditolak"
    );
}
