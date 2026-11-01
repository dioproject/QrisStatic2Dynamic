use crate::domain::amount::{FeeConfig, Rupiah, UniqueCodeConfig, calculate_final_amount};
use crate::domain::image::decode_qr_from_image_bytes;
use crate::domain::qris::{parse_qris, transform_to_dynamic};
use crate::domain::render::{EccLevel, render_qr_png_base64, render_qr_svg};
use crate::error::AppError;
use axum::{Json, body::Bytes, extract::State, http::HeaderMap, response::IntoResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ParseRequest {
    pub payload: Option<String>,
    #[serde(default)]
    pub include_tlv: bool,
}

pub async fn parse_handler(
    State(_state): State<crate::state::AppState>,
    headers: HeaderMap,
    body_bytes: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    // 1. Tangani JSON
    if content_type.starts_with("application/json") {
        let req: ParseRequest = serde_json::from_slice(&body_bytes)
            .map_err(|e| AppError::Internal(format!("JSON request tidak valid: {}", e)))?;
        if let Some(payload) = req.payload {
            let res = parse_qris(&payload, req.include_tlv)?;
            return Ok(Json(res));
        }
        return Err(AppError::QrisEmpty);
    }

    // 2. Tangani Multipart (Upload Gambar)
    if content_type.starts_with("multipart/form-data") {
        let boundary =
            multer::parse_boundary(content_type).map_err(|_| AppError::QrisImageUnsupported)?;
        let mut multipart = multer::Multipart::new(
            futures_util::stream::once(async move { Ok::<_, std::io::Error>(body_bytes) }),
            boundary,
        );

        let mut image_bytes = None;
        let mut include_tlv = false;

        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
        {
            let name = field.name().unwrap_or_default().to_string();
            if name == "image" || name == "file" {
                let mut data = Vec::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|e| AppError::Internal(e.to_string()))?
                {
                    data.extend_from_slice(&chunk);
                }
                image_bytes = Some(data);
            } else if name == "include_tlv" {
                let text = field.text().await.unwrap_or_default();
                include_tlv = text.eq_ignore_ascii_case("true") || text == "1";
            }
        }

        if let Some(bytes) = image_bytes {
            let payload = tokio::task::spawn_blocking(move || decode_qr_from_image_bytes(&bytes))
                .await
                .map_err(|e| AppError::Internal(e.to_string()))??;

            let res = parse_qris(&payload, include_tlv)?;
            return Ok(Json(res));
        }
    }

    Err(AppError::QrisEmpty)
}

#[derive(Debug, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_true")]
    pub png: bool,
    #[serde(default)]
    pub svg: bool,
    #[serde(default = "default_scale")]
    pub scale: u32,
    #[serde(default = "default_ecc")]
    pub ecc: String,
}

fn default_true() -> bool {
    true
}
fn default_scale() -> u32 {
    8
}
fn default_ecc() -> String {
    "M".to_string()
}

#[derive(Debug, Deserialize)]
pub struct DynamicRequest {
    pub payload: String,
    pub amount: i64,
    #[serde(default)]
    pub fee: FeeConfig,
    #[serde(default)]
    pub unique_code: UniqueCodeConfig,
    #[serde(default)]
    pub reference_label: Option<String>,
    #[serde(default)]
    pub output: Option<OutputConfig>,
}

#[derive(Debug, Serialize)]
pub struct QrOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub png_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DynamicResponse {
    pub payload: String,
    pub crc: String,
    pub payload_sha256: String,
    pub point_of_initiation: String,
    pub amount: crate::domain::amount::AmountBreakdown,
    pub merchant: crate::domain::qris::MerchantIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr: Option<QrOutput>,
}

pub async fn dynamic_handler(
    State(_state): State<crate::state::AppState>,
    Json(req): Json<DynamicRequest>,
) -> Result<impl IntoResponse, AppError> {
    let breakdown =
        calculate_final_amount(Rupiah::new(req.amount)?, &req.fee, req.unique_code.value)?;
    let dynamic_payload = transform_to_dynamic(&req.payload, breakdown.final_amount)?;

    let parsed = parse_qris(&dynamic_payload, false)?;

    let qr = if let Some(out_cfg) = req.output {
        let ecc = match out_cfg.ecc.to_uppercase().as_str() {
            "L" => EccLevel::L,
            "Q" => EccLevel::Q,
            "H" => EccLevel::H,
            _ => EccLevel::M,
        };

        let mut png_b64 = None;
        let mut svg_out = None;

        let p_clone1 = dynamic_payload.clone();
        if out_cfg.png {
            png_b64 = Some(
                tokio::task::spawn_blocking(move || {
                    render_qr_png_base64(&p_clone1, out_cfg.scale, ecc)
                })
                .await
                .map_err(|e| AppError::Internal(e.to_string()))??,
            );
        }

        let p_clone2 = dynamic_payload.clone();
        if out_cfg.svg {
            svg_out = Some(
                tokio::task::spawn_blocking(move || render_qr_svg(&p_clone2, ecc))
                    .await
                    .map_err(|e| AppError::Internal(e.to_string()))??,
            );
        }

        Some(QrOutput {
            png_base64: png_b64,
            svg: svg_out,
        })
    } else {
        None
    };

    Ok(Json(DynamicResponse {
        payload: dynamic_payload,
        crc: parsed.crc,
        payload_sha256: parsed.payload_sha256,
        point_of_initiation: parsed.point_of_initiation,
        amount: breakdown,
        merchant: parsed.merchant,
        qr,
    }))
}
