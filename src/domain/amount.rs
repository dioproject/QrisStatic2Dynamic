use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rupiah(pub i64);

impl Rupiah {
    pub const ZERO: Self = Self(0);
    pub const MIN_TRANSACTION: Self = Self(1);
    pub const MAX_TRANSACTION: Self = Self(999_999_999_999); // 1 Triliun minus 1
    pub const MAX_TAG54: Self = Self(9_999_999_999_999); // Batas EMVCo 13 digit

    pub fn new(val: i64) -> Result<Self, AppError> {
        if val < 0 {
            return Err(AppError::QrisAmountInvalid);
        }
        if val > Self::MAX_TAG54.0 {
            return Err(AppError::QrisAmountTooLarge);
        }
        Ok(Self(val))
    }

    pub fn value(&self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, AppError> {
        self.0
            .checked_add(other.0)
            .ok_or(AppError::QrisAmountTooLarge)
            .and_then(Self::new)
    }
}

impl fmt::Display for Rupiah {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for Rupiah {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Rupiah {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FeeMode {
    None,
    Fixed,
    Percent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    pub mode: FeeMode,
    #[serde(default)]
    pub fixed: i64,
    #[serde(default)]
    pub percent_bps: i32,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            mode: FeeMode::None,
            fixed: 0,
            percent_bps: 0,
        }
    }
}

impl FeeConfig {
    pub fn calculate(&self, base: Rupiah) -> Result<Rupiah, AppError> {
        match self.mode {
            FeeMode::None => Ok(Rupiah::ZERO),
            FeeMode::Fixed => {
                if self.fixed < 0 {
                    return Err(AppError::QrisAmountInvalid);
                }
                Rupiah::new(self.fixed)
            }
            FeeMode::Percent => {
                if self.percent_bps < 0 || self.percent_bps > 10_000 {
                    return Err(AppError::QrisAmountInvalid);
                }
                // integer floor division
                let fee = (base.0 as i128 * self.percent_bps as i128 / 10_000) as i64;
                Rupiah::new(fee)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UniqueCodeConfig {
    pub enabled: bool,
    #[serde(default)]
    pub value: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmountBreakdown {
    pub base: i64,
    pub fee: i64,
    pub unique_code: i32,
    pub final_amount: i64,
    pub currency: &'static str,
}

pub fn calculate_final_amount(
    base: Rupiah,
    fee_cfg: &FeeConfig,
    unique_code: Option<i32>,
) -> Result<AmountBreakdown, AppError> {
    if base.0 < Rupiah::MIN_TRANSACTION.0 || base.0 > Rupiah::MAX_TRANSACTION.0 {
        return Err(AppError::QrisAmountInvalid);
    }

    let fee = fee_cfg.calculate(base)?;
    let u_code = unique_code.unwrap_or(0);
    if !(0..=999).contains(&u_code) {
        return Err(AppError::QrisAmountInvalid);
    }

    let final_val = base
        .checked_add(fee)?
        .checked_add(Rupiah::new(u_code as i64)?)?;

    Ok(AmountBreakdown {
        base: base.0,
        fee: fee.0,
        unique_code: u_code,
        final_amount: final_val.0,
        currency: "IDR",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_none() {
        let base = Rupiah(500_000);
        let cfg = FeeConfig::default();
        let res = calculate_final_amount(base, &cfg, None).unwrap();
        assert_eq!(res.final_amount, 500_000);
        assert_eq!(res.fee, 0);
        assert_eq!(res.unique_code, 0);
    }

    #[test]
    fn test_fee_fixed_plus_unique_code() {
        let base = Rupiah(500_000);
        let cfg = FeeConfig {
            mode: FeeMode::Fixed,
            fixed: 2500,
            percent_bps: 0,
        };
        let res = calculate_final_amount(base, &cfg, Some(142)).unwrap();
        assert_eq!(res.base, 500_000);
        assert_eq!(res.fee, 2500);
        assert_eq!(res.unique_code, 142);
        assert_eq!(res.final_amount, 502_642);
    }

    #[test]
    fn test_fee_percent_floor() {
        let base = Rupiah(100_050);
        let cfg = FeeConfig {
            mode: FeeMode::Percent,
            fixed: 0,
            percent_bps: 70, // 0.7% = 700.35 -> floor 700
        };
        let res = calculate_final_amount(base, &cfg, None).unwrap();
        assert_eq!(res.fee, 700);
        assert_eq!(res.final_amount, 100_750);
    }
}
