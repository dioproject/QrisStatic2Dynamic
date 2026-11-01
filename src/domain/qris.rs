use crate::domain::crc16::crc16_hex;
use crate::domain::tlv::{TlvTag, encode_tlv, parse_nested_tlv, parse_tlv};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerchantIdentity {
    pub name: String,
    pub city: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    pub mcc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nmid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquirer_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merchant_criteria: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrisParsed {
    pub valid: bool,
    pub is_dynamic: bool,
    pub payload: String,
    pub payload_sha256: String,
    pub crc: String,
    pub currency: String,
    pub country: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    pub point_of_initiation: String,
    pub merchant: MerchantIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tlv: Option<Vec<TlvTag>>,
}

/// Validasi mendalam dan parsing payload QRIS statis maupun dinamis
pub fn parse_qris(payload: &str, include_tlv: bool) -> Result<QrisParsed, AppError> {
    if payload.is_empty() {
        return Err(AppError::QrisEmpty);
    }
    if !payload.is_ascii() {
        return Err(AppError::QrisNotAscii);
    }
    if payload.len() > 4096 {
        return Err(AppError::QrisTooLong);
    }
    if !payload.starts_with("000201") {
        return Err(AppError::QrisInvalidPrefix);
    }

    let tags = parse_tlv(payload)?;

    // Tag wajib: 00, 01, 53, 58, 59, 60, 63
    let get_tag = |t_name: &str| tags.iter().find(|t| t.tag == t_name);

    let tag00 = get_tag("00").ok_or(AppError::QrisMissingTag("00"))?;
    if tag00.value != "01" {
        return Err(AppError::QrisInvalidTagValue(
            "Tag 00 harus bernilai '01'".to_string(),
        ));
    }

    let tag01 = get_tag("01").ok_or(AppError::QrisMissingTag("01"))?;
    let poi = tag01.value.clone();
    let is_dynamic = match poi.as_str() {
        "11" => false,
        "12" => true,
        _ => {
            return Err(AppError::QrisInvalidTagValue(
                "Tag 01 harus bernilai '11' atau '12'".to_string(),
            ));
        }
    };

    let tag53 = get_tag("53").ok_or(AppError::QrisMissingTag("53"))?;
    if tag53.value != "360" {
        return Err(AppError::QrisUnsupportedCurrency(tag53.value.clone()));
    }

    let tag58 = get_tag("58").ok_or(AppError::QrisMissingTag("58"))?;
    if tag58.value != "ID" {
        return Err(AppError::QrisUnsupportedCountry(tag58.value.clone()));
    }

    let tag59 = get_tag("59").ok_or(AppError::QrisMissingTag("59"))?;
    let tag60 = get_tag("60").ok_or(AppError::QrisMissingTag("60"))?;
    let tag63 = get_tag("63").ok_or(AppError::QrisMissingTag("63"))?;

    // Verifikasi CRC-16
    let without_crc = &payload[..payload.len() - 4];
    let calculated_crc = crc16_hex(without_crc.as_bytes());
    if !tag63.value.eq_ignore_ascii_case(&calculated_crc) {
        return Err(AppError::QrisInvalidCrc {
            expected: calculated_crc,
            actual: tag63.value.clone(),
        });
    }

    // Periksa tag fee 55, 56, 57 (dilarang)
    if tags
        .iter()
        .any(|t| t.tag == "55" || t.tag == "56" || t.tag == "57")
    {
        return Err(AppError::QrisUnsupportedFeeTags);
    }

    // MCC (tag 52)
    let mcc = get_tag("52")
        .map(|t| t.value.clone())
        .unwrap_or_else(|| "0000".to_string());
    let postal_code = get_tag("61").map(|t| t.value.clone());

    // Amount (tag 54) jika ada
    let amount = if let Some(t54) = get_tag("54") {
        let val: i64 = t54.value.parse().map_err(|_| AppError::QrisAmountInvalid)?;
        Some(val)
    } else {
        None
    };

    // Ekstraksi Identitas Merchant & NMID (ERP §7.5)
    let mut nmid = None;
    let mut acquirer_guid = None;
    let mut merchant_criteria = None;

    // Cari di semua tag 26..=51
    let mut candidates = Vec::new();
    for t in &tags {
        if let Ok(num) = t.tag.parse::<u8>()
            && (26..=51).contains(&num)
            && let Ok(sub_tags) = parse_nested_tlv(&t.tag, &t.value)
        {
            let mut guid = None;
            let mut mid = None;
            let mut criteria = None;
            for st in &sub_tags {
                match st.tag.as_str() {
                    "00" => guid = Some(st.value.clone()),
                    "02" => mid = Some(st.value.clone()),
                    "03" => criteria = Some(st.value.clone()),
                    _ => {}
                }
            }
            candidates.push((guid, mid, criteria));
        }
    }

    // 1. Utamakan akun dengan sub-tag 00 berisi ID.CO.QRIS dan sub-tag 02 cocok regex NMID ^ID[0-9]{13,15}$
    for (guid, mid, crit) in &candidates {
        if let (Some(g), Some(m)) = (guid, mid)
            && g.contains("ID.CO.QRIS")
            && m.starts_with("ID")
            && (15..=17).contains(&m.len())
        {
            acquirer_guid = Some(g.clone());
            nmid = Some(m.clone());
            merchant_criteria = crit.clone();
            break;
        }
    }

    // 2. Fallback: akun pertama dengan format NMID ID...
    if nmid.is_none() {
        for (guid, mid, crit) in &candidates {
            if let Some(m) = mid
                && m.starts_with("ID")
                && (15..=17).contains(&m.len())
            {
                acquirer_guid = guid.clone();
                nmid = Some(m.clone());
                merchant_criteria = crit.clone();
                break;
            }
        }
    }

    // 3. Fallback jika masih kosong ambil dari kandidat pertama
    if acquirer_guid.is_none() && !candidates.is_empty() {
        acquirer_guid = candidates[0].0.clone();
        if nmid.is_none() {
            nmid = candidates[0].1.clone();
        }
        if merchant_criteria.is_none() {
            merchant_criteria = candidates[0].2.clone();
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    let payload_sha256 = format!("{:x}", hasher.finalize());

    Ok(QrisParsed {
        valid: true,
        is_dynamic,
        payload: payload.to_string(),
        payload_sha256,
        crc: tag63.value.to_uppercase(),
        currency: "IDR".to_string(),
        country: "ID".to_string(),
        amount,
        point_of_initiation: poi,
        merchant: MerchantIdentity {
            name: tag59.value.clone(),
            city: tag60.value.clone(),
            postal_code,
            mcc,
            nmid,
            acquirer_guid,
            merchant_criteria,
        },
        tlv: if include_tlv { Some(tags) } else { None },
    })
}

/// Transformasi QRIS statis ke dinamis secara MERCHANT-SAFE
/// Hanya tag 01, 54, dan 63 yang dimutasi.
/// Tag 26-51, 52, 53, 58, 59, 60, 61, 62, 64 tetap byte-identik.
pub fn transform_to_dynamic(payload: &str, final_amount: i64) -> Result<String, AppError> {
    if final_amount <= 0 {
        return Err(AppError::QrisAmountInvalid);
    }
    if final_amount > 9_999_999_999_999 {
        return Err(AppError::QrisAmountTooLarge);
    }

    let parsed_initial = parse_qris(payload, true)?;
    let tags = parsed_initial.tlv.unwrap();

    // 1. Buat body tanpa tag 54 dan tag 63
    let mut body: Vec<TlvTag> = tags
        .into_iter()
        .filter(|t| t.tag != "54" && t.tag != "63")
        .collect();

    // 2. Ubah tag 01 dari "11" -> "12"
    for t in &mut body {
        if t.tag == "01" && t.value == "11" {
            t.value = "12".to_string();
            t.length = 2;
        }
    }

    // 3. Sisipkan tag 54 tepat setelah tag 53
    let amount_str = final_amount.to_string();
    let tag54 = TlvTag::new("54", amount_str);

    let insert_idx = if let Some(pos) = body.iter().position(|t| t.tag == "53") {
        pos + 1
    } else if let Some(pos) = body.iter().position(|t| t.tag == "58") {
        pos
    } else {
        body.len()
    };

    body.insert(insert_idx, tag54);

    // 4. Encode TLV
    let encoded = encode_tlv(&body);
    let without_crc = format!("{}6304", encoded);
    let new_crc = crc16_hex(without_crc.as_bytes());
    let dynamic_payload = format!("{}{}", without_crc, new_crc);

    // 5. Verifikasi Invariant Merchant-Safe (RULES.md §1 & §15.6)
    assert_merchant_safe_invariants(payload, &dynamic_payload, final_amount)?;

    Ok(dynamic_payload)
}

fn assert_merchant_safe_invariants(
    original: &str,
    transformed: &str,
    expected_amount: i64,
) -> Result<(), AppError> {
    let orig_tags = parse_tlv(original)?;
    let new_tags = parse_tlv(transformed)?;

    let orig_map: std::collections::HashMap<_, _> = orig_tags
        .iter()
        .map(|t| (t.tag.clone(), t.value.clone()))
        .collect();
    let new_map: std::collections::HashMap<_, _> = new_tags
        .iter()
        .map(|t| (t.tag.clone(), t.value.clone()))
        .collect();

    // Pastikan tag 54 ada dan nilainya tepat expected_amount
    let t54_val = new_map
        .get("54")
        .ok_or_else(|| AppError::InvariantViolation("Tag 54 tidak ada pada output".to_string()))?;
    if t54_val != &expected_amount.to_string() {
        return Err(AppError::InvariantViolation(format!(
            "Tag 54 tidak sesuai: expected {}, got {}",
            expected_amount, t54_val
        )));
    }

    // Pastikan tag 01 bernilai "12"
    let t01_val = new_map
        .get("01")
        .ok_or_else(|| AppError::InvariantViolation("Tag 01 tidak ada pada output".to_string()))?;
    if t01_val != "12" {
        return Err(AppError::InvariantViolation(
            "Tag 01 bukan '12'".to_string(),
        ));
    }

    // Pastikan semua tag non-mutasi byte-identik
    for (tag, orig_val) in orig_map {
        if tag != "01" && tag != "54" && tag != "63" {
            let new_val = new_map.get(&tag).ok_or_else(|| {
                AppError::InvariantViolation(format!("Tag non-mutasi '{}' hilang pada output", tag))
            })?;
            if new_val != &orig_val {
                return Err(AppError::InvariantViolation(format!(
                    "Tag non-mutasi '{}' berubah nilainya!",
                    tag
                )));
            }
        }
    }

    // Verifikasi CRC output
    let without_crc = &transformed[..transformed.len() - 4];
    let calc_crc = crc16_hex(without_crc.as_bytes());
    let out_crc = &transformed[transformed.len() - 4..];
    if !out_crc.eq_ignore_ascii_case(&calc_crc) {
        return Err(AppError::InvariantViolation(
            "CRC output tidak valid!".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden Vector dari RULES.md §15.2
    const SYNTHETIC_STATIC: &str = "00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB";
    const SYNTHETIC_DYNAMIC_500K: &str = "00020101021226600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI52045812530336054065000005802ID5908KOS SAYA6007JAKARTA63040B8C";
    const SYNTHETIC_DYNAMIC_500142: &str = "00020101021226600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI52045812530336054065001425802ID5908KOS SAYA6007JAKARTA63048653";

    #[test]
    fn test_parse_golden_vector() {
        let parsed = parse_qris(SYNTHETIC_STATIC, true).unwrap();
        assert!(parsed.valid);
        assert!(!parsed.is_dynamic);
        assert_eq!(parsed.merchant.name, "KOS SAYA");
        assert_eq!(parsed.merchant.city, "JAKARTA");
        assert_eq!(parsed.merchant.nmid.as_deref(), Some("ID1024001234567"));
        assert_eq!(parsed.crc, "4CDB");
    }

    #[test]
    fn test_transform_dynamic_500000() {
        let dyn_res = transform_to_dynamic(SYNTHETIC_STATIC, 500_000).unwrap();
        assert_eq!(dyn_res, SYNTHETIC_DYNAMIC_500K);
    }

    #[test]
    fn test_transform_dynamic_500142() {
        let dyn_res = transform_to_dynamic(SYNTHETIC_STATIC, 500_142).unwrap();
        assert_eq!(dyn_res, SYNTHETIC_DYNAMIC_500142);
    }

    #[test]
    fn test_transform_idempotence() {
        let first = transform_to_dynamic(SYNTHETIC_STATIC, 500_142).unwrap();
        let second = transform_to_dynamic(&first, 500_142).unwrap();
        assert_eq!(first, second);
    }
}
