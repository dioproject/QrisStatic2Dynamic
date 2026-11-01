use qris_api::domain::crc16::crc16_hex;
use qris_api::domain::image::decode_qr_from_image_bytes;
use qris_api::domain::qris::{parse_qris, transform_to_dynamic};

const SYNTHETIC_STATIC: &str = "00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB";
const SYNTHETIC_DYNAMIC_500K: &str = "00020101021226600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI52045812530336054065000005802ID5908KOS SAYA6007JAKARTA63040B8C";
const SYNTHETIC_DYNAMIC_500142: &str = "00020101021226600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI52045812530336054065001425802ID5908KOS SAYA6007JAKARTA63048653";

#[test]
fn test_golden_vector_crc_and_transforms() {
    assert_eq!(crc16_hex(b"123456789"), "29B1");

    let parsed = parse_qris(SYNTHETIC_STATIC, true).expect("Harus valid");
    assert_eq!(parsed.crc, "4CDB");
    assert_eq!(parsed.merchant.name, "KOS SAYA");
    assert_eq!(parsed.merchant.nmid.as_deref(), Some("ID1024001234567"));

    let dyn_500k = transform_to_dynamic(SYNTHETIC_STATIC, 500_000).expect("Transform 500k sukses");
    assert_eq!(dyn_500k, SYNTHETIC_DYNAMIC_500K);

    let dyn_500142 =
        transform_to_dynamic(SYNTHETIC_STATIC, 500_142).expect("Transform 500142 sukses");
    assert_eq!(dyn_500142, SYNTHETIC_DYNAMIC_500142);
}

/// Pengujian fixture asli lokal (diabaikan secara default untuk CI publik)
/// Dijalankan secara lokal dengan: QRIS_REAL_FIXTURE_DIR=. cargo test -- --ignored
#[test]
#[ignore]
fn test_real_fixtures_local() {
    let fix_dir = std::env::var("QRIS_REAL_FIXTURE_DIR").unwrap_or_else(|_| ".".to_string());
    let path1 = std::path::Path::new(&fix_dir).join("Qris Static.jpeg");
    let path2 = std::path::Path::new(&fix_dir).join("Qris Static 2.jpeg");

    if path1.exists() {
        let bytes1 = std::fs::read(&path1).expect("Baca fixture 1");
        let payload1 = decode_qr_from_image_bytes(&bytes1).expect("Decode fixture 1");
        let parsed1 = parse_qris(&payload1, true).expect("Parse fixture 1");
        assert_eq!(parsed1.crc, "B676");
        let dyn1 = transform_to_dynamic(&payload1, 500_142).expect("Transform fixture 1");
        assert!(dyn1.ends_with("A1E6"));
    }

    if path2.exists() {
        let bytes2 = std::fs::read(&path2).expect("Baca fixture 2");
        let payload2 = decode_qr_from_image_bytes(&bytes2).expect("Decode fixture 2");
        let parsed2 = parse_qris(&payload2, true).expect("Parse fixture 2");
        assert_eq!(parsed2.crc, "F145");
        let dyn2 = transform_to_dynamic(&payload2, 500_142).expect("Transform fixture 2");
        assert!(dyn2.ends_with("B20F"));
        // Pastikan tag 62 tetap utuh
        assert!(dyn2.contains("62070703A01"));
    }
}
