//! Implementasi CRC-16/CCITT-FALSE (EMVCo QRIS compliant)
//! Polynomial: 0x1021
//! Initial Value: 0xFFFF
//! RefIn: false, RefOut: false
//! XorOut: 0x0000

pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

pub fn crc16_hex(data: &[u8]) -> String {
    format!("{:04X}", crc16_ccitt_false(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_vector_123456789() {
        // Standar baku CCITT-FALSE: "123456789" -> 0x29B1
        let input = b"123456789";
        assert_eq!(crc16_hex(input), "29B1");
        assert_eq!(crc16_ccitt_false(input), 0x29B1);
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(crc16_ccitt_false(b""), 0xFFFF);
    }
}
