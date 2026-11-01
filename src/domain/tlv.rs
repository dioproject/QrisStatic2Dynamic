use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlvTag {
    pub tag: String,
    pub length: usize,
    pub value: String,
}

impl TlvTag {
    pub fn new(tag: impl Into<String>, value: impl Into<String>) -> Self {
        let tag = tag.into();
        let value = value.into();
        let length = value.len();
        Self { tag, length, value }
    }

    pub fn encode(&self) -> String {
        format!("{}{:02}{}", self.tag, self.length, self.value)
    }
}

/// Parse stream TLV flat sesuai standar EMVCo QR Code
pub fn parse_tlv(payload: &str) -> Result<Vec<TlvTag>, AppError> {
    if payload.is_empty() {
        return Err(AppError::QrisEmpty);
    }
    if !payload.is_ascii() {
        return Err(AppError::QrisNotAscii);
    }
    if payload.len() > 4096 {
        return Err(AppError::QrisTooLong);
    }

    let bytes = payload.as_bytes();
    let mut i = 0;
    let mut tags = Vec::new();
    let mut seen_tags = HashSet::new();

    while i < bytes.len() {
        if i + 4 > bytes.len() {
            return Err(AppError::QrisInvalidTlv(
                "Format tag atau panjang terpotong".to_string(),
            ));
        }

        let tag = match std::str::from_utf8(&bytes[i..i + 2]) {
            Ok(s) => s.to_string(),
            Err(_) => return Err(AppError::QrisNotAscii),
        };

        let len_str = match std::str::from_utf8(&bytes[i + 2..i + 4]) {
            Ok(s) => s,
            Err(_) => return Err(AppError::QrisNotAscii),
        };

        let len: usize = len_str.parse().map_err(|_| {
            AppError::QrisInvalidTlv(format!("Panjang tag '{}' bukan angka: {}", tag, len_str))
        })?;

        i += 4;
        if i + len > bytes.len() {
            return Err(AppError::QrisInvalidTlv(format!(
                "Panjang value tag '{}' melebihi sisa payload",
                tag
            )));
        }

        let val = match std::str::from_utf8(&bytes[i..i + len]) {
            Ok(s) => s.to_string(),
            Err(_) => return Err(AppError::QrisNotAscii),
        };

        // Aturan VR-05: Tag duplikat dilarang
        if seen_tags.contains(&tag) {
            return Err(AppError::QrisDuplicateTag(tag));
        }
        seen_tags.insert(tag.clone());

        tags.push(TlvTag {
            tag,
            length: len,
            value: val,
        });

        i += len;
    }

    Ok(tags)
}

/// Parse nested TLV untuk sub-tag akun pembayaran (tag 26-51, 62, 64)
pub fn parse_nested_tlv(parent_tag: &str, value: &str) -> Result<Vec<TlvTag>, AppError> {
    let bytes = value.as_bytes();
    let mut i = 0;
    let mut sub_tags = Vec::new();
    let mut seen_sub = HashSet::new();

    while i < bytes.len() {
        if i + 4 > bytes.len() {
            return Err(AppError::QrisInvalidNestedTlv {
                tag: parent_tag.to_string(),
                reason: "Format sub-tag atau panjang terpotong".to_string(),
            });
        }

        let tag = std::str::from_utf8(&bytes[i..i + 2])
            .map_err(|_| AppError::QrisNotAscii)?
            .to_string();

        let len_str =
            std::str::from_utf8(&bytes[i + 2..i + 4]).map_err(|_| AppError::QrisNotAscii)?;
        let len: usize = len_str
            .parse()
            .map_err(|_| AppError::QrisInvalidNestedTlv {
                tag: parent_tag.to_string(),
                reason: format!("Panjang sub-tag '{}' bukan angka", tag),
            })?;

        i += 4;
        if i + len > bytes.len() {
            return Err(AppError::QrisInvalidNestedTlv {
                tag: parent_tag.to_string(),
                reason: format!("Panjang value sub-tag '{}' melebihi sisa data", tag),
            });
        }

        let val = std::str::from_utf8(&bytes[i..i + len])
            .map_err(|_| AppError::QrisNotAscii)?
            .to_string();

        if seen_sub.contains(&tag) {
            return Err(AppError::QrisInvalidNestedTlv {
                tag: parent_tag.to_string(),
                reason: format!("Sub-tag '{}' terduplikasi", tag),
            });
        }
        seen_sub.insert(tag.clone());

        sub_tags.push(TlvTag {
            tag,
            length: len,
            value: val,
        });

        i += len;
    }

    Ok(sub_tags)
}

/// Encode daftar TLV menjadi string
pub fn encode_tlv(tags: &[TlvTag]) -> String {
    let mut out = String::new();
    for t in tags {
        out.push_str(&t.encode());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tlv_roundtrip() {
        let original = "00020101021153033605802ID";
        let tags = parse_tlv(original).unwrap();
        assert_eq!(tags.len(), 4);
        assert_eq!(tags[0].tag, "00");
        assert_eq!(tags[0].value, "01");
        assert_eq!(tags[1].tag, "01");
        assert_eq!(tags[1].value, "11");
        let encoded = encode_tlv(&tags);
        assert_eq!(encoded, original);
    }

    #[test]
    fn test_tlv_duplicate_rejected() {
        let dup = "000201000201";
        assert!(matches!(parse_tlv(dup), Err(AppError::QrisDuplicateTag(_))));
    }
}
