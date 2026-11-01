use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentDirection {
    Credit, // Dana masuk (CR)
    Debit,  // Dana keluar (DR)
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ParsedNotification {
    pub amount: Option<i64>,
    pub direction: PaymentDirection,
    pub masked_text: String,
    pub parser_version: &'static str,
    pub match_reason: &'static str,
}

static REGEX_AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
    // Menangkap format: Rp 500.142, Rp500.142,00, IDR 500,142.00, 500.142
    Regex::new(r"(?i)(?:rp\.?|idr)\s*([\d\.,]+)").expect("Regex amount valid")
});

static REGEX_MASK_DIGITS: LazyLock<Regex> = LazyLock::new(|| {
    // Masking rekening / nomor telepon (deret >= 8 digit yang bukan bagian dari nominal)
    Regex::new(r"\b\d{8,}\b").expect("Regex mask valid")
});

pub fn parse_notification_text(raw_text: &str) -> ParsedNotification {
    let lower = raw_text.to_lowercase();

    // 1. Deteksi Direction (RULES.md §10 PR-03 & PR-04)
    let has_credit = lower.contains("masuk")
        || lower.contains("diterima")
        || lower.contains("menerima")
        || lower.contains("kredit")
        || lower.contains("cr");

    let has_debit = lower.contains("keluar")
        || lower.contains("debit")
        || lower.contains("db")
        || lower.contains("mengirim")
        || lower.contains("dikirim")
        || lower.contains("pembayaran ke")
        || lower.contains("transfer ke")
        || lower.contains("penarikan");

    let direction = match (has_credit, has_debit) {
        (true, false) => PaymentDirection::Credit,
        (false, true) => PaymentDirection::Debit,
        _ => PaymentDirection::Unknown,
    };

    // 2. Ekstraksi Nominal
    let mut amount = None;
    let mut match_reason = "no_amount_found";

    if let Some(caps) = REGEX_AMOUNT.captures(&lower)
        && let Some(matched) = caps.get(1)
    {
        let mut num_str = matched.as_str().replace(['.', ','], "");
        if (matched.as_str().ends_with(",00") || matched.as_str().ends_with(".00"))
            && num_str.len() > 2
        {
            num_str.truncate(num_str.len() - 2);
        }

        if let Ok(val) = num_str.parse::<i64>()
            && (1..=1_000_000_000_000).contains(&val)
        {
            amount = Some(val);
            match_reason = "regex_amount_match";
        }
    }

    // 3. Masking Teks Mentah (RULES.md §13 DR-02 & DR-03)
    let masked_text = REGEX_MASK_DIGITS.replace_all(raw_text, "****").to_string();

    ParsedNotification {
        amount,
        direction,
        masked_text,
        parser_version: "v1.0.0",
        match_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gopay_merchant() {
        let text = "Pembayaran QRIS Rp 500.142 berhasil diterima.";
        let res = parse_notification_text(text);
        assert_eq!(res.amount, Some(500_142));
        assert_eq!(res.direction, PaymentDirection::Credit);
    }

    #[test]
    fn test_parse_bca_email() {
        let text = "Dana masuk Rp500.142,00 pada 01/11/2026. Rekening 1234567890.";
        let res = parse_notification_text(text);
        assert_eq!(res.amount, Some(500_142));
        assert_eq!(res.direction, PaymentDirection::Credit);
        assert!(res.masked_text.contains("****"));
        assert!(!res.masked_text.contains("1234567890"));
    }

    #[test]
    fn test_parse_debit_ignored() {
        let text = "Pembayaran ke Toko ABC sebesar Rp 50.000 berhasil.";
        let res = parse_notification_text(text);
        assert_eq!(res.direction, PaymentDirection::Debit);
    }
}
