use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    // QRIS Domain Errors
    #[error("Payload QRIS kosong.")]
    QrisEmpty,

    #[error("Payload QRIS memuat karakter non-ASCII.")]
    QrisNotAscii,

    #[error("Panjang payload QRIS melebihi batas 4096 byte.")]
    QrisTooLong,

    #[error("Payload QRIS tidak diawali prefix '000201'.")]
    QrisInvalidPrefix,

    #[error("Struktur TLV QRIS rusak: {0}")]
    QrisInvalidTlv(String),

    #[error("Struktur TLV bersarang pada tag {tag} rusak: {reason}")]
    QrisInvalidNestedTlv { tag: String, reason: String },

    #[error("Tag QRIS terduplikasi: {0}")]
    QrisDuplicateTag(String),

    #[error("Tag wajib QRIS tidak ditemukan: {0}")]
    QrisMissingTag(&'static str),

    #[error("Nilai tag QRIS tidak valid: {0}")]
    QrisInvalidTagValue(String),

    #[error("Checksum CRC16 QRIS tidak cocok.")]
    QrisInvalidCrc { expected: String, actual: String },

    #[error("Mata uang QRIS tidak didukung (wajib 360/IDR).")]
    QrisUnsupportedCurrency(String),

    #[error("Negara QRIS tidak didukung (wajib ID).")]
    QrisUnsupportedCountry(String),

    #[error("QRIS statis memuat tag fee 55/56/57 yang dilarang.")]
    QrisUnsupportedFeeTags,

    #[error("Nominal QRIS tidak valid.")]
    QrisAmountInvalid,

    #[error("Nominal QRIS melebihi batas maksimal (13 digit).")]
    QrisAmountTooLarge,

    // Image Errors
    #[error("Format file gambar tidak didukung.")]
    QrisImageUnsupported,

    #[error("Ukuran file gambar melebihi batas 2 MB.")]
    QrisImageTooLarge,

    #[error("Dimensi gambar melebihi batas 2048x2048 piksel.")]
    QrisImageDimensionsExceeded,

    #[error("File gambar rusak atau tidak dapat dibaca.")]
    QrisImageInvalid,

    #[error("Gagal melakukan decode gambar.")]
    QrisImageDecodeFailed,

    #[error("Kode QR tidak ditemukan di dalam gambar.")]
    QrisQrNotFound,

    #[error("Ditemukan lebih dari satu kode QR di dalam gambar.")]
    QrisMultipleQr,

    // Amount & Fee Errors
    #[error("Nominal transaksi bentrok dengan transaksi PENDING lain.")]
    TxnAmountCollision,

    #[error("Pool kode unik (1..=999) untuk merchant ini telah habis.")]
    UniqueCodePoolExhausted,

    #[error("Kode unik yang diminta sedang digunakan oleh transaksi PENDING lain.")]
    UniqueCodeUnavailable,

    // Invariant Failure (Internal Server Error)
    #[error("Pelanggaran merchant-safe invariant: {0}")]
    InvariantViolation(String),

    // General Internal Error
    #[error("Terjadi kesalahan internal pada server.")]
    Internal(String),

    // Auth Errors (RULES.md §14)
    #[error("Header Authorization tidak ditemukan.")]
    AuthMissing,

    #[error("Kredensial atau API key tidak valid.")]
    AuthInvalid,

    #[error("API key sudah dicabut (revoked).")]
    AuthKeyRevoked,

    #[error("API key telah kedaluwarsa.")]
    AuthKeyExpired,

    #[error("Akses ditolak: izin tidak mencukupi.")]
    AuthForbidden,

    #[error("Signed request diwajibkan (header tanda tangan kurang).")]
    SignatureRequired,

    #[error("Tanda tangan HMAC tidak valid.")]
    SignatureInvalid,

    #[error("Timestamp tanda tangan di luar batas toleransi clock skew.")]
    SignatureTimestampInvalid,

    #[error("Nonce tanda tangan sudah pernah digunakan (replay attack).")]
    SignatureReplayed,

    #[error("Merchant tidak aktif atau ditangguhkan.")]
    MerchantInactive,

    #[error("Merchant tidak ditemukan.")]
    MerchantNotFound,

    #[error("Payload QRIS sudah terdaftar pada merchant lain.")]
    MerchantAlreadyRegistered,

    #[error("NMID sudah terdaftar pada merchant lain.")]
    MerchantNmidConflict,

    #[error("Transaksi tidak ditemukan.")]
    TxnNotFound,

    #[error("Status transaksi bukan PENDING.")]
    TxnNotPending,

    #[error("Transaksi sudah berstatus PAID.")]
    TxnAlreadyPaid,

    #[error("Transaksi sudah kedaluwarsa.")]
    TxnExpired,

    #[error("Idempotency-Key sama namun body request berbeda.")]
    TxnIdempotencyConflict,

    #[error("TTL transaksi di luar batas yang diizinkan (60..=86400).")]
    TxnTtlInvalid,
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::QrisEmpty => "QRIS_EMPTY",
            Self::QrisNotAscii => "QRIS_NOT_ASCII",
            Self::QrisTooLong => "QRIS_TOO_LONG",
            Self::QrisInvalidPrefix => "QRIS_INVALID_PREFIX",
            Self::QrisInvalidTlv(_) => "QRIS_INVALID_TLV",
            Self::QrisInvalidNestedTlv { .. } => "QRIS_INVALID_NESTED_TLV",
            Self::QrisDuplicateTag(_) => "QRIS_DUPLICATE_TAG",
            Self::QrisMissingTag(_) => "QRIS_MISSING_TAG",
            Self::QrisInvalidTagValue(_) => "QRIS_INVALID_TAG_VALUE",
            Self::QrisInvalidCrc { .. } => "QRIS_INVALID_CRC",
            Self::QrisUnsupportedCurrency(_) => "QRIS_UNSUPPORTED_CURRENCY",
            Self::QrisUnsupportedCountry(_) => "QRIS_UNSUPPORTED_COUNTRY",
            Self::QrisUnsupportedFeeTags => "QRIS_UNSUPPORTED_FEE_TAGS",
            Self::QrisAmountInvalid => "QRIS_AMOUNT_INVALID",
            Self::QrisAmountTooLarge => "QRIS_AMOUNT_TOO_LARGE",
            Self::QrisImageUnsupported => "QRIS_IMAGE_UNSUPPORTED",
            Self::QrisImageTooLarge => "QRIS_IMAGE_TOO_LARGE",
            Self::QrisImageDimensionsExceeded => "QRIS_IMAGE_DIMENSIONS_EXCEEDED",
            Self::QrisImageInvalid => "QRIS_IMAGE_INVALID",
            Self::QrisImageDecodeFailed => "QRIS_IMAGE_DECODE_FAILED",
            Self::QrisQrNotFound => "QRIS_QR_NOT_FOUND",
            Self::QrisMultipleQr => "QRIS_MULTIPLE_QR",
            Self::TxnAmountCollision => "TXN_AMOUNT_COLLISION",
            Self::UniqueCodePoolExhausted => "UNIQUE_CODE_POOL_EXHAUSTED",
            Self::UniqueCodeUnavailable => "UNIQUE_CODE_UNAVAILABLE",
            Self::InvariantViolation(_) => "INTERNAL_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::AuthMissing => "AUTH_MISSING",
            Self::AuthInvalid => "AUTH_INVALID",
            Self::AuthKeyRevoked => "AUTH_KEY_REVOKED",
            Self::AuthKeyExpired => "AUTH_KEY_EXPIRED",
            Self::AuthForbidden => "AUTH_FORBIDDEN",
            Self::SignatureRequired => "SIGNATURE_REQUIRED",
            Self::SignatureInvalid => "SIGNATURE_INVALID",
            Self::SignatureTimestampInvalid => "SIGNATURE_TIMESTAMP_INVALID",
            Self::SignatureReplayed => "SIGNATURE_REPLAYED",
            Self::MerchantInactive => "MERCHANT_INACTIVE",
            Self::MerchantNotFound => "MERCHANT_NOT_FOUND",
            Self::MerchantAlreadyRegistered => "MERCHANT_ALREADY_REGISTERED",
            Self::MerchantNmidConflict => "MERCHANT_NMID_CONFLICT",
            Self::TxnNotFound => "TXN_NOT_FOUND",
            Self::TxnNotPending => "TXN_NOT_PENDING",
            Self::TxnAlreadyPaid => "TXN_ALREADY_PAID",
            Self::TxnExpired => "TXN_EXPIRED",
            Self::TxnIdempotencyConflict => "TXN_IDEMPOTENCY_CONFLICT",
            Self::TxnTtlInvalid => "TXN_TTL_INVALID",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::QrisImageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::QrisImageUnsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::TxnAmountCollision
            | Self::UniqueCodePoolExhausted
            | Self::UniqueCodeUnavailable
            | Self::MerchantAlreadyRegistered
            | Self::MerchantNmidConflict
            | Self::TxnNotPending
            | Self::TxnAlreadyPaid
            | Self::TxnExpired
            | Self::TxnIdempotencyConflict => StatusCode::CONFLICT,
            Self::AuthMissing
            | Self::AuthInvalid
            | Self::AuthKeyRevoked
            | Self::AuthKeyExpired
            | Self::SignatureRequired
            | Self::SignatureInvalid
            | Self::SignatureTimestampInvalid
            | Self::SignatureReplayed => StatusCode::UNAUTHORIZED,
            Self::AuthForbidden | Self::MerchantInactive => StatusCode::FORBIDDEN,
            Self::MerchantNotFound | Self::TxnNotFound => StatusCode::NOT_FOUND,
            Self::InvariantViolation(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    pub fn details(&self) -> Option<serde_json::Value> {
        match self {
            Self::QrisInvalidCrc { expected, actual } => Some(serde_json::json!({
                "expected": expected,
                "actual": actual,
            })),
            Self::QrisInvalidNestedTlv { tag, reason } => Some(serde_json::json!({
                "tag": tag,
                "reason": reason,
            })),
            Self::QrisDuplicateTag(tag) => Some(serde_json::json!({
                "tag": tag,
            })),
            Self::QrisMissingTag(tag) => Some(serde_json::json!({
                "tag": tag,
            })),
            _ => None,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code().to_string(),
                message: self.to_string(),
                details: self.details(),
                request_id: None,
            },
        };
        (status, Json(body)).into_response()
    }
}
