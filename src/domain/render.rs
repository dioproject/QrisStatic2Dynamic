use crate::error::AppError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::{ImageBuffer, Luma};
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use std::io::Cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EccLevel {
    L,
    #[default]
    M,
    Q,
    H,
}

impl From<EccLevel> for EcLevel {
    fn from(level: EccLevel) -> Self {
        match level {
            EccLevel::L => EcLevel::L,
            EccLevel::M => EcLevel::M,
            EccLevel::Q => EcLevel::Q,
            EccLevel::H => EcLevel::H,
        }
    }
}

/// Render payload QRIS menjadi SVG string
pub fn render_qr_svg(payload: &str, ecc: EccLevel) -> Result<String, AppError> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), ecc.into())
        .map_err(|e| AppError::Internal(format!("Gagal render QR SVG: {}", e)))?;

    let svg_str = code
        .render()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();

    Ok(svg_str)
}

/// Render payload QRIS menjadi PNG bytes
pub fn render_qr_png(payload: &str, scale: u32, ecc: EccLevel) -> Result<Vec<u8>, AppError> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), ecc.into())
        .map_err(|e| AppError::Internal(format!("Gagal generate QR: {}", e)))?;

    let img: ImageBuffer<Luma<u8>, Vec<u8>> = code
        .render::<Luma<u8>>()
        .min_dimensions(scale * 29, scale * 29)
        .build();

    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| AppError::Internal(format!("Gagal encode PNG: {}", e)))?;

    Ok(buf.into_inner())
}

/// Render payload QRIS menjadi PNG base64 string
pub fn render_qr_png_base64(payload: &str, scale: u32, ecc: EccLevel) -> Result<String, AppError> {
    let png_bytes = render_qr_png(payload, scale, ecc)?;
    Ok(BASE64.encode(png_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_svg_and_png() {
        let payload = "00020101021153033605802ID6304XXXX";
        let svg = render_qr_svg(payload, EccLevel::M).unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));

        let png = render_qr_png(payload, 8, EccLevel::M).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

        let b64 = render_qr_png_base64(payload, 8, EccLevel::M).unwrap();
        assert!(!b64.is_empty());
    }
}
