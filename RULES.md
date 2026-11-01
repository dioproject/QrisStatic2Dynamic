# RULES — Business, Security & Data Rules — QrisStatic2Dynamic

> Versi: 1.0 · Status: Final · Tanggal: 2026-11-01
> Dokumen ini **normatif**. Jika kode dan dokumen ini berbeda, kode dianggap bug.
> Gunakan kata **WAJIB** (MUST), **SEBAIKNYA** (SHOULD), dan **BOLEH** (MAY) secara harfiah.
> Dokumen terkait: [PRD](./PRD.md) · [ERP](./ERP.md) · [AGENTS](./AGENTS.md) · [README](./README.md)

---

## 0. Urutan Prioritas

Jika terjadi konflik antar aturan:

1. **Merchant-Safe Invariants** (§1) — tidak boleh dilanggar dalam kondisi apa pun.
2. **Keamanan** (§8–§10) — tidak boleh dilonggarkan demi kenyamanan.
3. **Kebenaran payload/uang** (§3–§6).
4. **Kontrak API** ([ERP](./ERP.md) §8–§9).
5. **Preferensi operasional** (§18).

Perubahan yang melanggar urutan ini wajib ditolak di code review.

---

## 1. Merchant-Safe Invariants (MS)

| ID | Aturan |
|---|---|
| `MS-01` | Hanya tag `01`, `54`, dan `63` yang boleh berubah. |
| `MS-02` | Seluruh tag `26–51` (merchant account information) WAJIB identik byte-per-byte dan urutannya tidak berubah. |
| `MS-03` | Tag `52` (MCC), `53` (currency), `58` (country), `59` (name), `60` (city), `61` (postal), dan `64` (language) WAJIB identik byte-per-byte. |
| `MS-04` | Tag `62` (Additional Data) WAJIB identik byte-per-byte. Dilarang menyisipkan bill number, reference, terminal label, atau sub-tag apa pun. |
| `MS-05` | Tag `55`, `56`, dan `57` (tip/fee EMV) DILARANG muncul di input dan DILARANG dibuat di output. Fee dihitung bisnis dan digabung ke tag `54`. |
| `MS-06` | Tag `54` hanya berisi digit rupiah bulat, tanpa desimal, tanpa pemisah ribuan, maksimal 13 digit. |
| `MS-07` | Tag `63` selalu dihitung ulang dan selalu menjadi 4 karakter terakhir. |
| `MS-08` | `reference_label`, `merchant_ref`, nomor invoice, dan metadata bisnis lain DILARANG masuk ke payload QRIS. |
| `MS-09` | Setelah transformasi, output WAJIB di-parse ulang dan diverifikasi terhadap seluruh invariant di atas. Jika gagal, kembalikan 500 dan jangan keluarkan payload. |
| `MS-10` | `point_of_initiation = "preserve"` hanya mempertahankan tag `01`; tidak boleh mengubah tag lain di luar `54/63`. |

Contoh pelanggaran yang harus ditolak:

- menambahkan `5405...` tanpa menghapus tag `54` lama → tag duplikat;
- menulis ulang tag `59` menjadi nama internal merchant;
- menyisipkan `0105INV01` ke dalam tag `62`;
- mengubah `5303360` menjadi `5303360` + tag `55`;
- membuang tag `26` atau `51` yang tidak dikenal.

---

## 2. Validasi Payload (VR)

| ID | Aturan |
|---|---|
| `VR-01` | Input di-trim hanya whitespace ASCII di awal/akhir. Whitespace/control character di tengah → `QRIS_INVALID_TLV`. |
| `VR-02` | Payload WAJIB ASCII, panjang 1–4096 byte. Kosong → `QRIS_EMPTY`; terlalu panjang → `QRIS_TOO_LONG`. |
| `VR-03` | Payload WAJIB diawali `000201`. Selain itu → `QRIS_INVALID_PREFIX`. |
| `VR-04` | Setiap TLV: tag 2 digit, length 2 digit desimal, value tepat sepanjang length. Tidak boleh ada sisa/truncation. |
| `VR-05` | Tag duplikat di level mana pun → `QRIS_DUPLICATE_TAG`. |
| `VR-06` | Tag `63` WAJIB ada, berada tepat di 4 karakter terakhir, dan bernilai 4 digit hex. |
| `VR-07` | Tag `00` WAJIB `01`. |
| `VR-08` | Tag `53` WAJIB `360`; tag `58` WAJIB `ID`. Selain itu → `QRIS_UNSUPPORTED_CURRENCY` / `QRIS_UNSUPPORTED_COUNTRY`. |
| `VR-09` | Tag `59` (merchant name), `60` (city), dan `63` WAJIB ada. Jika tidak → `QRIS_MISSING_TAG`. |
| `VR-10` | Tag `01`, jika ada, hanya boleh `11` atau `12`. Nilai lain → `QRIS_INVALID_TAG_VALUE`. |
| `VR-11` | Template `26–51`, `62`, dan `64` WAJIB merupakan TLV bersarang yang valid. Gagal → `QRIS_INVALID_NESTED_TLV`. |
| `VR-12` | Tag `55`, `56`, `57` di input → `QRIS_UNSUPPORTED_FEE_TAGS`. |
| `VR-13` | Tag `54`, jika ada, hanya boleh digit dan > 0. Nilai tidak valid → `QRIS_AMOUNT_INVALID`. |
| `VR-14` | Panjang tag `54` ≤ 13 digit; tag `59` ≤ 25 karakter; tag `60` ≤ 15; tag `61` ≤ 10. |
| `VR-15` | Tag yang tidak dikenal dipertahankan, tetapi strukturnya tetap harus valid. |
| `VR-16` | CRC dihitung dengan CRC-16/CCITT-FALSE: poly `0x1021`, init `0xFFFF`, no reflect, xorout `0x0000`, atas seluruh byte sebelum 4 karakter CRC (termasuk `6304`). |
| `VR-17` | Perbandingan CRC case-insensitive; output selalu uppercase. |
| `VR-18` | Payload yang sudah dinamis (punya tag `54`/`01=12`) BOLEH diproses; tag `54` lama diganti, dan response memuat warning `QRIS_AMOUNT_REPLACED`. |
| `VR-19` | Validasi WAJIB dilakukan sebelum transformasi; jangan pernah "memperbaiki" payload rusak secara diam-diam. |
| `VR-20` | Untuk input gambar: WAJIB cek magic bytes (`PNG`, `JPEG`, `WEBP`), ukuran ≤ 2 MB, dimensi ≤ 2048×2048, dan decode di `spawn_blocking`. |

---

## 3. Transformasi (TR)

Algoritma wajib, berurutan:

```
1. validate(payload) sesuai §2
2. tlv = parse(payload)
3. tolak jika ada tag 55/56/57
4. body = semua tag kecuali 54 dan 63, urutan dipertahankan
5. jika mode = dynamic dan tag 01 ada dan nilainya 11 -> ubah menjadi 12
   jika mode = preserve -> biarkan tag 01
6. sisipkan tag 54 = digits(final_amount) tepat setelah tag 53
   fallback: sebelum tag 58
   fallback: sebelum tag 63
7. encoded = encode_tlv(body)
8. tanpa_crc = encoded + "6304"
9. crc = crc16_ccitt_false(tanpa_crc)
10. hasil = tanpa_crc + uppercase(crc)
11. assert_invariants(hasil)
```

| ID | Aturan |
|---|---|
| `TR-01` | Tag `54` lama WAJIB dihapus sebelum disisipkan ulang; tidak boleh ada dua tag `54`. |
| `TR-02` | Tag `63` lama WAJIB dihapus; CRC tidak pernah diwarisi. |
| `TR-03` | Posisi tag selain `54/63` tidak boleh berubah. |
| `TR-04` | Jika tag `53` tidak ada, validasi sudah menolak; fallback posisi hanya untuk payload yang lolos validasi namun tidak lazim. |
| `TR-05` | `final_amount` diformat dengan `to_string()` Rust (tanpa leading zero, tanpa pemisah). |
| `TR-06` | `assert_invariants` WAJIB membandingkan setiap tag non-mutasi dengan hasil parse awal, termasuk urutan. |
| `TR-07` | Transformasi dengan nominal yang sama WAJIB idempoten: `transform(transform(p)) == transform(p)`. |
| `TR-08` | Output WAJIB dapat di-decode ulang dan menghasilkan tag yang sama secara semantik. |
| `TR-09` | `reference_label` dan `merchant_ref` tidak boleh memengaruhi payload. |
| `TR-10` | Jangan pernah memilih "tag terakhir" atau "tag pertama" secara asal untuk `54`; gunakan posisi standar di atas. |
| `TR-11` | Jangan melakukan normalisasi/uppercase pada nilai tag selain CRC. |

---

## 4. Nominal, Fee, dan Kode Unik (AM)

| ID | Aturan |
|---|---|
| `AM-01` | Semua nominal adalah integer rupiah (`i64`). Dilarang `f64`/desimal di jalur mana pun. |
| `AM-02` | `base_amount` WAJIB `1 ..= 999_999_999_999`. |
| `AM-03` | `fee_mode` hanya `NONE`, `FIXED`, atau `PERCENT` (eksklusif). |
| `AM-04` | `FIXED` memakai `fee_fixed` (`0 ..= 999_999_999`); `PERCENT` memakai `fee_percent_bps` (`0 ..= 10_000`). |
| `AM-05` | Fee persen dihitung `floor(base_amount * bps / 10_000)`. Tidak ada pembulatan ke atas. |
| `AM-06` | `fee_amount` tidak boleh negatif dan tidak boleh melebihi `base_amount × 100%`. |
| `AM-07` | `unique_code` = `0` (nonaktif) atau `1 ..= 999`. |
| `AM-08` | `final_amount = base_amount + fee_amount + unique_code`. |
| `AM-09` | `final_amount` WAJIB `> 0` dan `≤ 9_999_999_999_999` (13 digit, batas tag `54`). |
| `AM-10` | Fee dan kode unik TIDAK boleh dikirim sebagai tag `55/56/57`; semuanya masuk tag `54`. |
| `AM-11` | Untuk transaksi stateful, kode unik default mengikuti merchant (`default_unique_code_enabled`, default `true`). |
| `AM-12` | Untuk `/qris/dynamic` stateless, kode unik default `false` kecuali diminta eksplisit. |
| `AM-13` | Alokasi kode unik otomatis WAJIB memakai `BEGIN IMMEDIATE`, kandidat acak CSPRNG, dan partial unique index sebagai backstop. |
| `AM-14` | Kode unik manual yang sudah dipakai transaksi `PENDING` → `409 UNIQUE_CODE_UNAVAILABLE`. |
| `AM-15` | Jika seluruh 999 kode sedang dipakai → `409 UNIQUE_CODE_POOL_EXHAUSTED`. |
| `AM-16` | Kode unik dilepas otomatis saat status tidak lagi `PENDING`; tidak ada reuse selama `PENDING`. |
| `AM-17` | Dua transaksi `PENDING` milik merchant yang sama DILARANG memiliki `final_amount` sama, bahkan jika kode unik nonaktif. Partial unique index wajib menegakkan ini; pelanggaran → `409 TXN_AMOUNT_COLLISION`. |
| `AM-18` | Response WAJIB memuat rincian `base`, `fee`, `unique_code`, dan `final` agar client dapat merekonsiliasi. |
| `AM-19` | Tidak ada biaya tersembunyi: fee yang dikenakan ke pembeli harus sama dengan yang dikembalikan API. |
| `AM-20` | Nominal final tidak boleh dibulatkan lagi oleh renderer QR. Renderer menerima string final yang sudah final. |

---

## 5. Siklus Transaksi (TX)

| ID | Aturan |
|---|---|
| `TX-01` | Transaksi hanya boleh dibuat untuk merchant `ACTIVE`. |
| `TX-02` | `ttl_seconds` WAJIB `60 ..= 86_400`; default merchant (900 detik). |
| `TX-03` | `expires_at = created_at + ttl_seconds × 1000` (epoch ms). |
| `TX-04` | Sweeper WAJIB mengubah `PENDING` yang `expires_at + grace < now` menjadi `EXPIRED`. `grace` default 300 detik untuk toleransi keterlambatan notifikasi. |
| `TX-05` | `cancel` hanya untuk `PENDING`; selain itu `409 TXN_NOT_PENDING`. |
| `TX-06` | `mark-paid` hanya untuk master, WAJIB menyertakan alasan 3–200 karakter, dan WAJIB diaudit. |
| `TX-07` | `mark-paid` BOLEH dari `PENDING`, `EXPIRED`, `CANCELED`, atau `AMBIGUOUS`; DILARANG dari `PAID` (`409 TXN_ALREADY_PAID`). |
| `TX-08` | Transisi status WAJIB atomik dengan guard status di `WHERE`. |
| `TX-09` | Transaksi `PAID` bersifat terminal dan tidak boleh dibatalkan/diubah nominalnya. |
| `TX-10` | Setiap transaksi `PAID` WAJIB menghasilkan tepat satu outbound delivery `transaction.paid` (jika callback URL ada). |
| `TX-11` | Merchant key hanya boleh membaca/mengubah transaksi milik merchant-nya; pelanggaran → `403 AUTH_FORBIDDEN`. |
| `TX-12` | QR PNG/SVG hanya dapat diakses dengan kredensial yang berhak; tidak ada endpoint publik berbasis ID. |
| `TX-13` | Response transaksi WAJIB memuat `status`, `expires_at`, dan rincian nominal. |

---

## 6. Idempotency (ID)

| ID | Aturan |
|---|---|
| `ID-01` | Key diambil dari header `Idempotency-Key` atau body `idempotency_key`; maksimal 128 karakter; hanya ASCII printable. |
| `ID-02` | Key unik per merchant; key yang sama di merchant lain adalah key berbeda. |
| `ID-03` | Hash body kanonik = SHA-256 dari raw body request (byte persis). |
| `ID-04` | Key + hash sama → kembalikan transaksi yang sama, `200`, header `Idempotent-Replay: true`. |
| `ID-05` | Key sama + hash beda → `409 TXN_IDEMPOTENCY_CONFLICT`. |
| `ID-06` | Request paralel dengan key sama: satu menang, lainnya membaca transaksi pemenang; partial unique index wajib mencegah duplikasi. |
| `ID-07` | Idempotency tidak berlaku untuk endpoint stateless. |
| `ID-08` | Key tidak boleh dipakai ulang untuk transaksi yang sudah terminal dengan body berbeda; ini tetap konflik. |

---

## 7. Autentikasi & Otorisasi (AU)

| ID | Aturan |
|---|---|
| `AU-01` | Master key berasal dari `MASTER_API_KEY`, minimal 32 karakter, dibandingkan constant-time (`subtle`). |
| `AU-02` | Master key sebelumnya (`MASTER_API_KEY_PREVIOUS`) BOLEH diterima selama masa rotasi; log warning setiap pemakaian. |
| `AU-03` | Merchant key berformat `qsk_live_` + 40 karakter base62; server hanya menyimpan SHA-256 dan prefix 16 karakter. |
| `AU-04` | Lookup merchant key memakai prefix; verifikasi hash WAJIB constant-time. |
| `AU-05` | Key `REVOKED`/expired → `401 AUTH_KEY_REVOKED`/`AUTH_KEY_EXPIRED`; pesan tidak boleh membocorkan apakah prefix ada. |
| `AU-06` | Merchant `SUSPENDED`/`DELETED` tidak boleh membuat transaksi baru → `403 MERCHANT_SUSPENDED`. |
| `AU-07` | Merchant key hanya boleh: stateless parse/dynamic, membaca profil sendiri, membuat/membaca/cancel transaksi sendiri. |
| `AU-08` | Master key boleh semua endpoint. |
| `AU-09` | Endpoint admin (merchant CRUD, key/source management, list semua transaksi, mark-paid, test-webhook) WAJIB master. |
| `AU-10` | API key DILARANG dikirim lewat query string, path, body, atau log. Hanya header `Authorization`. |
| `AU-11` | Registrasi merchant dan rotasi key WAJIB menampilkan key/secret sekali; setelah itu tidak ada endpoint yang mengembalikannya. |
| `AU-12` | Rotasi key/secret WAJIB atomik: buat baru, revoke lama, dalam satu transaksi DB. |
| `AU-13` | Hanya satu signing secret `ACTIVE` per merchant. |
| `AU-14` | Source secret hanya ditampilkan sekali dan disimpan terenkripsi; revoke berlaku seketika. |
| `AU-15` | `ALLOW_PUBLIC_STATELESS` default `false`. Jika `true`, endpoint stateless boleh tanpa key, tetapi rate limit per IP lebih ketat dan tidak ada data yang disimpan. |
| `AU-16` | Kegagalan auth WAJIB uniform dan tidak membocorkan perbedaan antara "key tidak ada" dan "key salah". |
| `AU-17` | `last_used_at` key diperbarui maksimal sekali per menit; jangan menulis DB di setiap request. |

---

## 8. Signed Request & Anti-Replay (SG)

Berlaku jika `REQUIRE_SIGNED_REQUESTS=true` (default produksi) untuk semua request
non-GET yang terautentikasi. Inbound webhook selalu memakai aturan §9.1.

| ID | Aturan |
|---|---|
| `SG-01` | Header wajib: `X-Qris-Timestamp`, `X-Qris-Nonce`, `X-Qris-Signature`. |
| `SG-02` | `X-Qris-Timestamp` = epoch detik. Skew maksimal `SIGNATURE_MAX_SKEW_SECONDS` (default 300). |
| `SG-03` | `X-Qris-Nonce` = 16–64 karakter ASCII, unik per kredensial. |
| `SG-04` | Kanonik: `timestamp + "\n" + nonce + "\n" + METHOD + "\n" + path_and_query + "\n" + lower_hex(sha256(raw_body))`. |
| `SG-05` | `path_and_query` adalah path + query persis seperti diterima, tanpa fragment, tanpa normalisasi urutan query. |
| `SG-06` | `raw_body` adalah byte body persis; body kosong memakai SHA-256 dari string kosong. |
| `SG-07` | Signature = `"v1=" + lower_hex(HMAC-SHA256(secret, kanonik))`. |
| `SG-08` | Master memakai `MASTER_API_KEY`; merchant memakai signing secret `ACTIVE`. |
| `SG-09` | Perbandingan signature WAJIB constant-time. |
| `SG-10` | Nonce disimpan di `request_nonces` dengan primary key `(credential_id, nonce)`; duplikat → `401 SIGNATURE_REPLAYED`. |
| `SG-11` | Nonce disimpan minimal `2 × skew` (default 600 detik), lalu dibersihkan worker. |
| `SG-12` | Timestamp di luar skew → `401 SIGNATURE_TIMESTAMP_INVALID`; signature salah → `401 SIGNATURE_INVALID`; header kurang → `401 SIGNATURE_REQUIRED`. |
| `SG-13` | Signature, nonce, dan secret DILARANG dicatat di log. |
| `SG-14` | Server WAJIB sinkron NTP; clock skew internal > 30 detik adalah insiden. |
| `SG-15` | Request GET tidak wajib signature, tetapi tetap wajib TLS dan Bearer key. |
| `SG-16` | Nonce tidak boleh dipakai ulang meskipun body/method berbeda. |

Contoh:

```
timestamp  = 1767225600
nonce      = 01J8Z7Y8Y8Y8Y8Y8Y8Y8Y8Y8Y8
method     = POST
path       = /api/v1/merchants/mch_01J.../transactions
body_hash  = 9f2c...e1
canonical  = "1767225600\n01J8...\nPOST\n/api/v1/merchants/mch_01J.../transactions\n9f2c...e1"
signature  = "v1=" + hmac_sha256_hex(secret, canonical)
```

---

## 9. Rate Limit, Upload & Webhook Security (RL)

| ID | Aturan |
|---|---|
| `RL-01` | Rate limit global per IP: default 120 request/menit, burst 60. |
| `RL-02` | Rate limit per kredensial: default 120 request/menit; endpoint admin 10/menit. |
| `RL-03` | Decode gambar: default 20 request/menit per kredensial/IP. |
| `RL-04` | Melebihi limit → `429 RATE_LIMITED` + header `Retry-After`. |
| `RL-05` | Body JSON maksimal 256 KB; multipart maksimal 2 MB; payload QRIS maksimal 4096 byte. |
| `RL-06` | Body melebihi batas → `413 PAYLOAD_TOO_LARGE`, tanpa membaca seluruh body ke memori. |
| `RL-07` | Upload hanya `image/png`, `image/jpeg`, `image/webp`; wajib cek magic bytes, bukan hanya `Content-Type`. |
| `RL-08` | Dimensi gambar maksimal 2048×2048 dan total piksel maksimal 4.194.304. |
| `RL-09` | Tolak gambar animasi/multi-frame dan gambar dengan rasio ekstrem. |
| `RL-10` | Decode gambar WAJIB di `spawn_blocking` dengan timeout; jangan memblokir runtime async. |
| `RL-11` | Jangan aktifkan decompression request body tanpa batas; jika aktif, batas hasil dekompresi sama dengan batas body. |
| `RL-12` | CORS hanya origin yang terdaftar di `CORS_ALLOWED_ORIGINS`; default deny. |
| `RL-13` | Header keamanan: `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store` untuk transaksi. |
| `RL-14` | Callback URL wajib HTTPS publik; tolak IP privat, loopback, link-local, multicast, dan redirect. |
| `RL-15` | Outbound webhook tidak boleh mengikuti redirect dan harus memakai timeout 10 detik. |
| `RL-16` | Jangan pernah mengeksekusi URL yang berasal dari body notifikasi; callback hanya dari konfigurasi merchant. |

### 9.1 Inbound webhook

| ID | Aturan |
|---|---|
| `WH-01` | Source harus `ACTIVE` dan merchant harus `ACTIVE`; jika tidak → 401/403. |
| `WH-02` | Signature diverifikasi SEBELUM body di-parse. |
| `WH-03` | `X-Qris-Event-Id` wajib, 1–128 karakter; unik per source. |
| `WH-04` | Kanonik inbound: `timestamp + "\n" + event_id + "\n" + lower_hex(sha256(raw_body))`. |
| `WH-05` | Timestamp di luar skew → 401; signature salah → 401. |
| `WH-06` | Body maksimal 64 KB. |
| `WH-07` | Android: `package` wajib ada dan harus ada di `allowed_packages`. |
| `WH-08` | Email: `from` wajib cocok dengan `allowed_senders` (alamat atau domain). |
| `WH-09` | Aggregator: jika `allowed_ips` diisi, IP sumber harus cocok. |
| `WH-10` | Event duplikat `(source_id, event_key)` → `202 DUPLICATE`, tanpa efek samping. |
| `WH-11` | Event yang tidak cocok tetap disimpan `UNMATCHED`; jangan membuang nominal yang berhasil diparse. |
| `WH-12` | `raw_text_masked` wajib dimasking; teks mentah tidak boleh disimpan/di-log. |
| `WH-13` | Update `last_seen_at` source maksimal sekali per menit. |
| `WH-14` | Event yang arahnya `DR` (debit/keluar) → `IGNORED`, tidak pernah match. |
| `WH-15` | Event tanpa nominal yang valid → `IGNORED` atau `ERROR` dengan alasan; bukan 500. |

---

## 10. Parser Notifikasi (PR)

| ID | Aturan |
|---|---|
| `PR-01` | Parser berjalan offline; dilarang memanggil API eksternal atau OCR cloud. |
| `PR-02` | Normalisasi teks: Unicode NFKC, lowercase untuk pencocokan keyword, tetapi angka dipertahankan. |
| `PR-03` | Keyword arah masuk: `masuk`, `diterima`, `menerima`, `kredit`, `cr`, `dana masuk`, `uang masuk`, `transfer masuk`, `qris masuk`. |
| `PR-04` | Keyword arah keluar: `keluar`, `debit`, `db`, `mengirim`, `dikirim`, `pembayaran ke`, `transfer ke`, `penarikan`. Jika keduanya ada, arah `UNKNOWN` dan event `IGNORED` kecuali source memberi `direction` eksplisit. |
| `PR-05` | Format nominal yang didukung: `Rp 500.142`, `Rp500.142`, `Rp 500.142,00`, `IDR 500,142.00`, `500.142`, `500142`. |
| `PR-06` | Pemisah ribuan Indonesia (`.`) dan desimal (`,`) dinormalisasi; desimal hanya boleh `,00`/`.00`. |
| `PR-07` | Jika ada lebih dari satu kandidat nominal berbeda, dan tidak ada yang jelas terkait keyword masuk, hasil = `AMBIGUOUS_AMOUNT` dan event `IGNORED`. Dilarang menebak. |
| `PR-08` | Nominal hasil parse wajib `1 ..= 1_000_000_000_000`; di luar itu `IGNORED`. |
| `PR-09` | Abaikan notifikasi OTP, promo, login, verifikasi perangkat, tagihan keluar, dan notifikasi yang tidak mengandung kata kunci transaksi. |
| `PR-10` | Regex wajib dibatasi (bounded quantifier) dan dikompilasi sekali (`LazyLock`); dilarang pola yang berpotensi catastrophic backtracking. |
| `PR-11` | Parser tidak boleh mengubah nominal yang sudah terstruktur dari aggregator kecuali validasi range. |
| `PR-12` | Setiap hasil parser menyimpan `parser_version` dan `match_reason` untuk audit. |
| `PR-13` | Teks notifikasi tidak boleh di-log; hanya `event_id`, `source_id`, status, dan nominal hasil parse. |

---

## 11. Matcher Pembayaran (MT)

| ID | Aturan |
|---|---|
| `MT-01` | Match hanya dalam lingkup merchant pemilik source. |
| `MT-02` | Kandidat hanya transaksi `status = PENDING`. |
| `MT-03` | Syarat waktu: `expires_at + grace > now`, grace default 300 detik. |
| `MT-04` | Match WAJIB exact pada `final_amount`. DILARANG match ke `base_amount`, `fee_amount`, atau nominal tanpa kode unik. |
| `MT-05` | Jika tepat satu kandidat → `PAID` secara atomik. |
| `MT-06` | Jika nol kandidat → event `UNMATCHED`, transaksi tidak berubah. |
| `MT-07` | Jika lebih dari satu kandidat (seharusnya dicegah index) → event `ERROR` dengan alasan `MULTIPLE_CANDIDATES`, tidak ada transaksi yang diubah, dan alarm operasional. |
| `MT-08` | Update status dan pembuatan outbound delivery WAJIB dalam satu transaksi DB (outbox pattern). |
| `MT-09` | SSE dipancarkan setelah commit, bukan sebelum. |
| `MT-10` | Dua event bersamaan dengan nominal sama: satu menang; yang kalah melihat tidak ada `PENDING` dan menjadi `UNMATCHED`. |
| `MT-11` | Event yang datang sebelum transaksi dibuat tetap `UNMATCHED`; v1 tidak melakukan auto-rematch. |
| `MT-12` | `mark-paid` manual melewati matcher, tetapi tetap mengisi `payment_source = MANUAL` dan audit. |
| `MT-13` | Transaksi `EXPIRED`/`CANCELED` tidak pernah cocok otomatis. |
| `MT-14` | Matcher tidak boleh melakukan fuzzy match berdasarkan nama pengirim, deskripsi, atau waktu. |

---

## 12. Outbound Webhook (OW)

| ID | Aturan |
|---|---|
| `OW-01` | Kirim hanya jika merchant punya `callback_url` dan event diaktifkan. |
| `OW-02` | Event: `transaction.paid`, `transaction.expired`, `transaction.canceled`, `merchant.test`. |
| `OW-03` | Signature memakai signing secret `ACTIVE`, kanonik sama dengan inbound: `timestamp + "\n" + event_id + "\n" + sha256(body)`. |
| `OW-04` | Retry: 6 percobaan, backoff 5s, 30s, 2m, 10m, 1h, 6h. |
| `OW-05` | Sukses hanya HTTP 2xx. 3xx tidak diikuti dan dianggap gagal. |
| `OW-06` | Timeout 10 detik; tidak ada retry untuk 4xx kecuali 408/429. |
| `OW-07` | `(transaction_id, event_type)` unik; event yang sama tidak dikirim dua kali. |
| `OW-08` | Payload disimpan sebagai snapshot di `webhook_deliveries`; jangan merakit ulang saat retry. |
| `OW-09` | Setelah 6 percobaan gagal → `DEAD`; tersedia untuk replay manual (v1.1). |
| `OW-10` | Payload tidak boleh memuat secret, API key, atau data notifikasi mentah. |
| `OW-11` | `merchant.test` tidak di-dedup dan tidak memengaruhi transaksi. |
| `OW-12` | Callback harus HTTPS publik; SSRF checklist §9 `RL-14`/`RL-15` wajib. |

---

## 13. Data & Retensi (DR)

| ID | Aturan |
|---|---|
| `DR-01` | API key hanya disimpan sebagai SHA-256; signing/source secret sebagai AES-256-GCM. |
| `DR-02` | Teks notifikasi hanya disimpan dalam bentuk masked; teks mentah dilarang di DB/log. |
| `DR-03` | Nomor rekening/telepon pada teks notifikasi dimasking sebelum disimpan. |
| `DR-04` | Transaksi dan audit disimpan minimal 365 hari; event notifikasi 90 hari; nonce 10 menit. |
| `DR-05` | Backup database diperlakukan sebagai secret dan disimpan terpisah. |
| `DR-06` | Merchant soft-delete tidak menghapus transaksi/audit. |
| `DR-07` | Jangan menyimpan data kartu, PIN, OTP, atau credential bank. |
| `DR-08` | Data yang tidak lagi diperlukan dihapus oleh worker retensi; penghapusan dicatat. |

---

## 14. Kode Error (ER)

Format envelope: lihat [ERP](./ERP.md) §8.4.

| Kode | HTTP | Kapan |
|---|---|---|
| `AUTH_MISSING` | 401 | Tidak ada header Authorization |
| `AUTH_INVALID` | 401 | Key tidak valid |
| `AUTH_KEY_REVOKED` | 401 | Key sudah direvoke |
| `AUTH_KEY_EXPIRED` | 401 | Key kedaluwarsa |
| `AUTH_FORBIDDEN` | 403 | Scope tidak cukup |
| `MERCHANT_SUSPENDED` | 403 | Merchant tidak aktif |
| `SIGNATURE_REQUIRED` | 401 | Signed request diwajibkan |
| `SIGNATURE_INVALID` | 401 | HMAC tidak cocok |
| `SIGNATURE_TIMESTAMP_INVALID` | 401 | Timestamp di luar skew |
| `SIGNATURE_REPLAYED` | 401 | Nonce sudah dipakai |
| `RATE_LIMITED` | 429 | Melebihi rate limit |
| `VALIDATION_ERROR` | 422 | Validasi umum |
| `PAYLOAD_TOO_LARGE` | 413 | Body melebihi batas |
| `UNSUPPORTED_MEDIA_TYPE` | 415 | Content-Type tidak didukung |
| `NOT_FOUND` | 404 | Resource tidak ada |
| `METHOD_NOT_ALLOWED` | 405 | Method salah |
| `CONFLICT` | 409 | Konflik umum |
| `INTERNAL_ERROR` | 500 | Error tak terduga |
| `SERVICE_UNAVAILABLE` | 503 | DB/ready gagal |
| `QRIS_EMPTY` | 422 | Payload kosong |
| `QRIS_NOT_ASCII` | 422 | Ada karakter non-ASCII |
| `QRIS_TOO_LONG` | 422 | Payload > 4096 byte |
| `QRIS_INVALID_PREFIX` | 422 | Tidak diawali `000201` |
| `QRIS_INVALID_TLV` | 422 | Struktur TLV rusak |
| `QRIS_INVALID_NESTED_TLV` | 422 | Template bersarang rusak |
| `QRIS_DUPLICATE_TAG` | 422 | Tag duplikat |
| `QRIS_MISSING_TAG` | 422 | Tag wajib tidak ada |
| `QRIS_INVALID_TAG_VALUE` | 422 | Nilai tag tidak valid |
| `QRIS_INVALID_CRC` | 422 | CRC tidak cocok |
| `QRIS_UNSUPPORTED_CURRENCY` | 422 | Tag 53 bukan 360 |
| `QRIS_UNSUPPORTED_COUNTRY` | 422 | Tag 58 bukan ID |
| `QRIS_UNSUPPORTED_FEE_TAGS` | 422 | Ada tag 55/56/57 |
| `QRIS_AMOUNT_INVALID` | 422 | Nominal tidak valid |
| `QRIS_AMOUNT_TOO_LARGE` | 422 | Nominal > batas tag 54 |
| `QRIS_IMAGE_INVALID` | 422 | Gambar rusak |
| `QRIS_IMAGE_TOO_LARGE` | 413 | Gambar > 2 MB |
| `QRIS_IMAGE_DIMENSIONS_EXCEEDED` | 422 | Dimensi/piksel terlalu besar |
| `QRIS_IMAGE_UNSUPPORTED` | 415 | Format gambar tidak didukung |
| `QRIS_IMAGE_DECODE_FAILED` | 422 | Gagal decode gambar |
| `QRIS_QR_NOT_FOUND` | 422 | Tidak ada QR di gambar |
| `QRIS_MULTIPLE_QR` | 422 | Lebih dari satu QR berbeda |
| `MERCHANT_NOT_FOUND` | 404 | Merchant tidak ada |
| `MERCHANT_ALREADY_REGISTERED` | 409 | Payload sudah terdaftar |
| `MERCHANT_NMID_CONFLICT` | 409 | NMID sudah terdaftar |
| `MERCHANT_INACTIVE` | 403 | Merchant suspended/deleted |
| `KEY_NOT_FOUND` | 404 | Key tidak ada |
| `SOURCE_NOT_FOUND` | 404 | Source tidak ada |
| `TXN_NOT_FOUND` | 404 | Transaksi tidak ada |
| `TXN_NOT_PENDING` | 409 | Transaksi bukan PENDING |
| `TXN_ALREADY_PAID` | 409 | Transaksi sudah PAID |
| `TXN_EXPIRED` | 409 | Transaksi sudah EXPIRED |
| `TXN_IDEMPOTENCY_CONFLICT` | 409 | Key sama, body beda |
| `TXN_AMOUNT_INVALID` | 422 | Nominal transaksi tidak valid |
| `TXN_AMOUNT_COLLISION` | 409 | `final_amount` bentrok dengan transaksi PENDING lain saat kode unik nonaktif |
| `TXN_TTL_INVALID` | 422 | TTL di luar batas |
| `UNIQUE_CODE_POOL_EXHAUSTED` | 409 | 999 kode aktif terpakai |
| `UNIQUE_CODE_UNAVAILABLE` | 409 | Kode manual sudah dipakai |
| `IDEMPOTENCY_KEY_INVALID` | 422 | Key tidak valid |
| `WEBHOOK_SOURCE_NOT_FOUND` | 404 | Source tidak ada |
| `WEBHOOK_SOURCE_REVOKED` | 401 | Source sudah direvoke |
| `WEBHOOK_EVENT_ID_REQUIRED` | 400 | Header event id tidak ada |
| `WEBHOOK_EVENT_ID_INVALID` | 422 | Event id tidak valid |
| `WEBHOOK_BODY_INVALID` | 422 | Body webhook tidak valid |
| `CALLBACK_URL_INVALID` | 422 | URL bukan HTTPS/publik |
| `CALLBACK_URL_PRIVATE` | 422 | URL mengarah ke jaringan privat |
| `WEBHOOK_DELIVERY_NOT_FOUND` | 404 | Delivery tidak ada |

Aturan pesan:
- Bahasa Indonesia, singkat, tidak menyalahkan pengguna secara kasar.
- Tidak memuat secret, stack trace, SQL, path internal, atau nama host DB.
- `details` hanya berisi field yang relevan dan aman.

---

## 15. Golden Vector & Acceptance Test (TV)

Semua vektor di bawah WAJIB ada sebagai unit test. Angka CRC dihitung dengan
CRC-16/CCITT-FALSE.

### 15.1 CRC dasar

| Input | Expected |
|---|---|
| `123456789` | `29B1` |

### 15.2 Payload statis sintetis (dua merchant account)

```
00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB
```

Ekspektasi parse:
- `valid = true`, `is_dynamic = false`
- merchant name `KOS SAYA`, city `JAKARTA`
- NMID `ID1024001234567` (dari tag `51`, **bukan** `000000001` dari tag `26`)
- `merchant_criteria = UMI`
- `acquirer_guid = ID.CO.QRIS.WWW`
- `mcc = 5812`, currency `IDR`, country `ID`
- `crc = 4CDB`

### 15.3 Konversi nominal 500000

```
00020101021226600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI52045812530336054065000005802ID5908KOS SAYA6007JAKARTA63040B8C
```

- `crc = 0B8C`
- tag `01 = 12`
- tag `54 = 500000`
- tag `26/51/52/53/58/59/60` identik.

### 15.4 Konversi nominal 500142 (base 500000 + kode unik 142)

```
00020101021226600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI52045812530336054065001425802ID5908KOS SAYA6007JAKARTA63048653
```

- `crc = 8653`
- tag `54 = 500142`.

### 15.5 Negatif

| Kasus | Expected |
|---|---|
| Ganti 4 karakter CRC menjadi `0000` | `QRIS_INVALID_CRC` |
| Prefix `999999` | `QRIS_INVALID_PREFIX` |
| Hapus tag `63` | `QRIS_MISSING_TAG` / `QRIS_INVALID_TLV` |
| Tambah tag `55` | `QRIS_UNSUPPORTED_FEE_TAGS` |
| Ubah tag `62` di input | Harus tetap utuh di output; bukan dilarang di input, tetapi tidak boleh dimutasi |
| Dua tag `54` | `QRIS_DUPLICATE_TAG` |
| Nominal `0` atau `-1` | `QRIS_AMOUNT_INVALID` |
| Nominal 14 digit | `QRIS_AMOUNT_TOO_LARGE` |
| Payload 4097 byte | `QRIS_TOO_LONG` |
| Gambar 5000×5000 | `QRIS_IMAGE_DIMENSIONS_EXCEEDED` |
| Gambar 3 MB | `QRIS_IMAGE_TOO_LARGE` |
| PNG palsu (magic bytes salah) | `QRIS_IMAGE_UNSUPPORTED`/`QRIS_IMAGE_INVALID` |

### 15.6 Invariant test

Untuk setiap vektor valid:

1. Parse output.
2. Bandingkan semua tag selain `01/54/63` dengan input; WAJIB identik dan urutannya sama.
3. Pastikan tidak ada `55/56/57`.
4. Hitung ulang CRC; WAJIB sama.
5. Decode ulang payload output; WAJIB menghasilkan tag yang sama.

### 15.7 Fixture QRIS asli

Repo memiliki dua fixture lokal untuk uji decode gambar:

- `Qris Static.jpeg`
- `Qris Static 2.jpeg`

Aturan:

- Fixture asli **tidak boleh** di-commit ke repositori publik tanpa persetujuan pemilik.
- Payload asli, NMID, nama merchant, dan kota **tidak boleh** dimasukkan ke dokumen publik,
  log, atau test snapshot yang di-commit.
- Test yang memakai fixture asli harus diberi tanda `#[ignore]` atau membaca direktori dari
  environment `QRIS_REAL_FIXTURE_DIR`, sehingga CI publik tetap hijau tanpa data merchant.
- Ekspektasi minimal:
  - Fixture 1: payload terbaca, CRC statis `B676`; transformasi 500142 menghasilkan CRC `A1E6`.
  - Fixture 2: payload terbaca, CRC statis `F145`; transformasi 500142 menghasilkan CRC `B20F`;
    tag `62` = `0703A01` WAJIB tetap utuh.
  - Pada kedua fixture, NMID diambil dari akun `ID.CO.QRIS` (tag `51`), bukan ID internal
    acquirer di tag `26`.
  - Semua tag identitas dipertahankan byte-per-byte.

---

## 16. Aturan Operasional (OP)

| ID | Aturan |
|---|---|
| `OP-01` | v1.0 hanya satu instance per database SQLite. Multi-instance butuh Postgres + nonce store terpusat. |
| `OP-02` | Bind default `127.0.0.1:8080`; exposure publik hanya lewat reverse proxy TLS. |
| `OP-03` | `MASTER_API_KEY` dan `DATA_ENCRYPTION_KEY` wajib berbeda dan disimpan di secret manager. |
| `OP-04` | Backup harian `VACUUM INTO`; verifikasi `PRAGMA integrity_check`; retensi 7 harian + 4 mingguan. |
| `OP-05` | Migrasi hanya additive; backup sebelum migrasi produksi. |
| `OP-06` | `/healthz` tidak boleh menyentuh DB; `/readyz` wajib cek `SELECT 1`. |
| `OP-07` | Log JSON terstruktur; level default `info`; `debug` tidak boleh memuat body/secret. |
| `OP-08` | Setiap request punya `X-Request-Id` (ULID) dan dicatat di audit bila relevan. |
| `OP-09` | Alert untuk: event `ERROR`/`MULTIPLE_CANDIDATES`, `DEAD` delivery, lonjakan `UNMATCHED`, auth failure, error 5xx. |
| `OP-10` | Rotasi master key: set `MASTER_API_KEY_PREVIOUS`, deploy baru, update client, lalu hapus key lama. |
| `OP-11` | Insiden kebocoran key: revoke key, rotate secret, audit transaksi 24 jam terakhir, beri tahu merchant. |
| `OP-12` | Jangan pernah menyalakan `ALLOW_PUBLIC_STATELESS` di produksi tanpa rate limit dan monitoring. |

---

## 17. Kepatuhan & Disclaimer

1. API ini **bukan** penyelenggara jasa pembayaran dan tidak menyimpan dana.
2. Merchant bertanggung jawab atas kepatuhan terhadap ketentuan acquirer/PTEN, ASPI,
   dan regulasi Bank Indonesia.
3. API tidak melakukan *surcharge* melalui tag EMV `55/56/57`; biaya layanan internal
   digabung ke nominal tag `54` dan harus transparan ke pembeli.
4. Data notifikasi dapat memuat data pribadi; masking dan retensi wajib dipatuhi
   (UU PDP dan kebijakan internal).
5. Penggunaan `point_of_initiation = "preserve"` adalah opsi kompatibilitas, bukan default;
   default tetap `12` (dinamis).
6. Uji pilot per acquirer sebelum produksi; tidak ada jaminan semua acquirer menerima
   payload yang dimodifikasi.

---

## 18. Definition of Rule Compliance

Sebuah perubahan dianggap mematuhi RULES jika:

- [ ] Semua golden vector §15 lulus.
- [ ] Test invariant merchant-safe lulus untuk minimal 100 payload acak/property test.
- [ ] Tidak ada tag `55/56/57` yang dibuat; input yang memuatnya ditolak.
- [ ] Tag `62` tidak pernah dimutasi.
- [ ] Semua nominal memakai integer dan CHECK DB.
- [ ] Auth, signature, nonce, rate limit, dan upload limit tidak dilonggarkan.
- [ ] Tidak ada secret/payload/teks notifikasi di log.
- [ ] Error baru terdaftar di §14 dan punya test.
- [ ] Dokumen PRD/ERP/RULES/README diperbarui jika perilaku berubah.
