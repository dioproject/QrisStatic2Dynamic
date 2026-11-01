use crate::error::AppError;
use image::GenericImageView;
use rqrr::PreparedImage;

pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024; // 2 MB
pub const MAX_IMAGE_PIXELS: u64 = 2048 * 2048; // 4.194.304 px

/// Decode kode QR dari raw bytes file gambar (PNG, JPEG, WebP)
/// Menjalankan validasi ketat: magic bytes, batas ukuran, dimensi, dan jumlah QR.
pub fn decode_qr_from_image_bytes(bytes: &[u8]) -> Result<String, AppError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(AppError::QrisImageTooLarge);
    }

    // Verifikasi magic bytes (RL-07)
    let is_png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let is_jpeg = bytes.starts_with(b"\xff\xd8\xff");
    let is_webp = bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP";

    if !is_png && !is_jpeg && !is_webp {
        return Err(AppError::QrisImageUnsupported);
    }

    let img = image::load_from_memory(bytes).map_err(|_| AppError::QrisImageInvalid)?;
    let (width, height) = img.dimensions();

    if (width as u64) * (height as u64) > MAX_IMAGE_PIXELS || width > 2048 || height > 2048 {
        return Err(AppError::QrisImageDimensionsExceeded);
    }

    // RL-09: tolak rasio ekstrem (mis. banner panjang) yang bukan QR wajar.
    let (w, h) = (width.max(1), height.max(1));
    if w.max(h) > w.min(h).saturating_mul(10) {
        return Err(AppError::QrisImageDimensionsExceeded);
    }

    // RL-09: tolak gambar multi-frame (WebP animasi).
    if is_webp && webp_is_animated(bytes) {
        return Err(AppError::QrisImageUnsupported);
    }

    // Konversi ke luma8 untuk rqrr
    let luma = img.to_luma8();
    let mut prepared = PreparedImage::prepare(luma);
    let grids = prepared.detect_grids();

    if grids.is_empty() {
        return Err(AppError::QrisQrNotFound);
    }

    let mut payloads = Vec::new();
    for grid in grids {
        let mut buf = Vec::new();
        if grid.decode_to(&mut buf).is_ok()
            && let Ok(content) = String::from_utf8(buf)
        {
            payloads.push(content);
        }
    }

    if payloads.is_empty() {
        return Err(AppError::QrisQrNotFound);
    }

    // Jika ada lebih dari satu QR dan isinya berbeda -> QrisMultipleQr
    let first = &payloads[0];
    for p in &payloads[1..] {
        if p != first {
            return Err(AppError::QrisMultipleQr);
        }
    }

    Ok(first.clone())
}

/// Mendeteksi WebP animasi dengan menelusuri chunk RIFF (RL-09).
///
/// WebP animasi memuat chunk `ANIM`; gambar statis tidak. Dekoder `image`
/// hanya mengambil frame pertama, sehingga tanpa cek ini animasi akan lolos.
fn webp_is_animated(bytes: &[u8]) -> bool {
    let mut pos = 12usize; // melewati "RIFF", size, "WEBP"

    while pos + 8 <= bytes.len() {
        let fourcc = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;

        if fourcc == b"ANIM" {
            return true;
        }

        let padded = size + (size & 1);
        pos = match pos.checked_add(8).and_then(|p| p.checked_add(padded)) {
            Some(next) => next,
            None => return false,
        };
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_bytes_rejection() {
        let fake = b"GIF89a...";
        assert!(matches!(
            decode_qr_from_image_bytes(fake),
            Err(AppError::QrisImageUnsupported)
        ));
    }

    #[test]
    fn test_detect_animated_webp() {
        // RIFF + size + WEBP + chunk ANIM.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&20u32.to_le_bytes());
        buf.extend_from_slice(b"WEBP");
        buf.extend_from_slice(b"ANIM");
        buf.extend_from_slice(&4u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]);
        assert!(webp_is_animated(&buf));

        // Tanpa chunk ANIM.
        let mut still = Vec::new();
        still.extend_from_slice(b"RIFF");
        still.extend_from_slice(&12u32.to_le_bytes());
        still.extend_from_slice(b"WEBP");
        assert!(!webp_is_animated(&still));
    }
}
