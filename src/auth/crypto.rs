use crate::error::AppError;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Master API Key perbandingan constant-time
pub fn verify_master_key(provided: &str, current: &str, previous: Option<&str>) -> bool {
    if provided.is_empty() {
        return false;
    }

    let mut valid = false;
    if !current.is_empty() && provided.as_bytes().ct_eq(current.as_bytes()).into() {
        valid = true;
    }
    if let Some(prev) = previous
        && !prev.is_empty()
        && provided.as_bytes().ct_eq(prev.as_bytes()).into()
    {
        valid = true;
    }
    valid
}

/// Hash API Key dengan SHA-256 (64 hex lowercase)
pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Enkripsi secret dengan AES-256-GCM + AAD
/// Output format: base64(nonce_12_bytes || ciphertext_and_tag)
pub fn encrypt_secret(secret: &str, dek_b64: &str, aad: &str) -> Result<String, AppError> {
    let key_bytes = BASE64
        .decode(dek_b64)
        .map_err(|_| AppError::Internal("DATA_ENCRYPTION_KEY invalid base64".to_string()))?;
    if key_bytes.len() != 32 {
        return Err(AppError::Internal(
            "DATA_ENCRYPTION_KEY wajib 32 byte".to_string(),
        ));
    }

    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| AppError::Internal(format!("AES cipher init gagal: {}", e)))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let payload = aes_gcm::aead::Payload {
        msg: secret.as_bytes(),
        aad: aad.as_bytes(),
    };

    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|e| AppError::Internal(format!("Enkripsi gagal: {}", e)))?;

    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(combined))
}

/// Dekripsi secret dengan AES-256-GCM + AAD
pub fn decrypt_secret(secret_enc: &str, dek_b64: &str, aad: &str) -> Result<String, AppError> {
    let key_bytes = BASE64
        .decode(dek_b64)
        .map_err(|_| AppError::Internal("DATA_ENCRYPTION_KEY invalid base64".to_string()))?;
    let raw = BASE64
        .decode(secret_enc)
        .map_err(|_| AppError::Internal("Encrypted payload invalid base64".to_string()))?;

    if raw.len() < 12 + 16 {
        return Err(AppError::Internal(
            "Encrypted payload terlalu pendek".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = raw.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| AppError::Internal(format!("AES cipher init gagal: {}", e)))?;

    let payload = aes_gcm::aead::Payload {
        msg: ciphertext,
        aad: aad.as_bytes(),
    };

    let plaintext = cipher.decrypt(nonce, payload).map_err(|_| {
        AppError::Internal("Gagal dekripsi: secret corrupted atau AAD tidak cocok".to_string())
    })?;

    String::from_utf8(plaintext)
        .map_err(|_| AppError::Internal("Decrypted text bukan UTF-8".to_string()))
}

/// Generator token acak URL-safe base62
pub fn generate_token(prefix: &str, len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let random_part: String = (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    format!("{}{}", prefix, random_part)
}
