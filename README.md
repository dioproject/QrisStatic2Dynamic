# QrisStatic2Dynamic

REST API **Rust + Axum** untuk mengubah **QRIS statis** menjadi **QRIS dinamis**
bernomial, dengan mode **stateless** maupun **stateful**, serta auto-detect pembayaran
dari notifikasi Android, email, dan aggregator mutasi.

> **Status:** M0 — spesifikasi final. Implementasi Rust dimulai pada milestone M1.
> Dokumen ini adalah kontrak target; perintah build di bawah akan tersedia setelah scaffold dibuat.

- **Merchant-safe:** hanya tag `01`, `54`, dan `63` yang berubah. Identitas merchant
  (`26–51`, `52`, `53`, `58`, `59`, `60`, `61`, `62`, `64`) tetap byte-identik.
- **Tanpa payment gateway:** dana masuk langsung ke rekening QRIS merchant.
- **Tanpa login/scraping bank:** auto-detect hanya membaca notifikasi yang diteruskan
  oleh merchant sendiri.
- **Aman untuk produksi:** API key berlapis, signed request + nonce, rate limit,
  validasi upload, audit log, dan redaksi log.

---

## 1. Fitur

| Fitur | Keterangan |
|---|---|
| Parse QRIS | Validasi EMVCo TLV, CRC16, ekstraksi merchant/NMID |
| Decode gambar | PNG/JPEG/WebP → payload QRIS (dibatasi 2 MB / 2048×2048) |
| Konversi dinamis | Tag `01` `11→12`, tag `54` disisipkan, CRC dihitung ulang |
| Render QR | PNG biner, SVG, atau base64 |
| Stateless API | Tanpa database; payload + nominal langsung jadi |
| Merchant stateful | Profil QRIS, API key, signing secret, callback URL |
| Fee | `NONE` / `FIXED` / `PERCENT`, digabung ke tag `54` |
| Kode unik | `1–999` per merchant, unik di antara transaksi `PENDING` |
| TTL & expiry | Transaksi kedaluwarsa otomatis + sweeper |
| Idempotency | `Idempotency-Key` mencegah transaksi ganda |
| Auto-detect | Android notification, email parser, aggregator (Moota/CekMutasi/custom) |
| Real-time | Server-Sent Events (SSE) per transaksi |
| Outbound webhook | `transaction.paid` bertanda tangan HMAC + retry |
| Keamanan | Master key, merchant key hash, AES-GCM secret, HMAC, nonce, rate limit |
| Audit | Aksi admin, mark-paid, rotasi key, perubahan status |

---

## 2. Arsitektur

```
┌──────────────┐      HTTPS       ┌──────────────────────────────┐
│ Client / POS │ ───────────────► │ Reverse proxy (TLS, Caddy)   │
│ Bot / Web    │                  └──────────────┬───────────────┘
└──────────────┘                                 │
                                                 ▼
                                    ┌─────────────────────────────┐
                                    │ Rust API (Axum + Tokio)     │
                                    │                             │
   Android forwarder ──HMAC──────►  │  auth → rate limit → body   │
   Email worker      ──HMAC──────►  │  → handler → service        │
   Aggregator        ──HMAC──────►  │                             │
                                    │  QR engine: TLV / CRC16     │
                                    │  image decode / QR render   │
                                    │  matcher + outbox worker    │
                                    └───────┬─────────────┬───────┘
                                            │             │
                                            ▼             ▼
                                     ┌───────────┐  ┌──────────────┐
                                     │ SQLite    │  │ SSE / outbound│
                                     │ WAL       │  │ webhook       │
                                     └───────────┘  └──────────────┘
```

---

## 3. Stack

| Lapisan | Teknologi |
|---|---|
| Bahasa | Rust stable ≥ 1.85, edition 2024 |
| Web | `axum` 0.8, `tokio`, `tower`, `tower-http` |
| Database | SQLite (WAL) via `sqlx` |
| QR decode | `image` + `rqrr` (pure Rust) |
| QR render | `qrcode` + `image` (PNG/SVG) |
| Kriptografi | `sha2`, `hmac`, `subtle`, `aes-gcm`, `rand` |
| Rate limit | `tower_governor` |
| HTTP client | `reqwest` (rustls) untuk outbound webhook |
| Observability | `tracing` + `tracing-subscriber` (JSON) |
| ID | ULID berprefix (`mch_`, `txn_`, `src_`, `evt_`, `dlv_`) |

---

## 4. Isi Repositori

```
QrisStatic2Dynamic/
├── PRD.md                  # kenapa & apa yang dibangun
├── ERP.md                  # ERD, DDL, kontrak JSON, signature
├── RULES.md                # aturan normatif + golden vector
├── AGENTS.md               # panduan agent/kontributor
├── README.md               # dokumen ini
├── Qris Static.jpeg        # fixture lokal (jangan commit ke repo publik)
└── Qris Static 2.jpeg      # fixture lokal (jangan commit ke repo publik)
```

Kode Rust, migrasi, dan test akan ditambahkan pada M1–M4 (lihat `AGENTS.md` §14).

---

## 5. Quickstart (target M1)

### 5.1 Prasyarat

- Rust stable ≥ 1.85
- SQLite 3.45+ (bundled oleh `sqlx`)
- `openssl` (untuk generate secret)

### 5.2 Konfigurasi

```bash
cp .env.example .env
```

Generate dua secret secara terpisah, lalu salin hasilnya ke `.env`
(jangan menulis `$(...)` di dalam file `.env`):

```bash
openssl rand -base64 48   # MASTER_API_KEY
openssl rand -base64 32   # DATA_ENCRYPTION_KEY
```

Isi minimal `.env`:

```env
MASTER_API_KEY=<hasil openssl rand -base64 48>
DATA_ENCRYPTION_KEY=<hasil openssl rand -base64 32>
DATABASE_URL=sqlite://./data/qris.db
APP_BIND=127.0.0.1:8080

# HANYA untuk lokal. Produksi wajib true dan memakai HMAC (ERP §8.3).
REQUIRE_SIGNED_REQUESTS=false
```

> `DATA_ENCRYPTION_KEY` **wajib** 32 byte setelah di-decode base64.
> Jangan pernah commit `.env` atau memakai placeholder di produksi.
> Untuk produksi set `REQUIRE_SIGNED_REQUESTS=true`; lihat checklist §10.

### 5.3 Jalankan

```bash
mkdir -p data
cargo run
```

Cek:

```bash
curl -s http://127.0.0.1:8080/healthz
# {"status":"ok","version":"0.1.0"}
```

### 5.4 Test

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

Test fixture asli (lokal, tidak untuk CI publik):

```bash
QRIS_REAL_FIXTURE_DIR=. cargo test -- --ignored
```

---

## 6. Environment Variables

| Variable | Default | Keterangan |
|---|---|---|
| `APP_ENV` | `development` | `development`/`production`; produksi menyembunyikan detail error |
| `APP_BIND` | `127.0.0.1:8080` | Alamat bind; jangan expose langsung tanpa proxy |
| `APP_BASE_URL` | `http://127.0.0.1:8080` | Dipakai untuk URL webhook source |
| `DATABASE_URL` | `sqlite://./data/qris.db` | Lokasi SQLite |
| `MASTER_API_KEY` | — | Wajib, ≥ 32 karakter |
| `MASTER_API_KEY_PREVIOUS` | — | Opsional, untuk rotasi tanpa downtime |
| `DATA_ENCRYPTION_KEY` | — | Wajib, base64 32 byte untuk AES-256-GCM |
| `ALLOW_PUBLIC_STATELESS` | `false` | Jika `true`, stateless tanpa key (tidak disarankan) |
| `REQUIRE_SIGNED_REQUESTS` | `true` | Wajib HMAC untuk request non-GET; quickstart lokal memakai `false` |
| `SIGNATURE_MAX_SKEW_SECONDS` | `300` | Toleransi timestamp |
| `RATE_LIMIT_PER_MINUTE` | `120` | Limit per IP/kredensial |
| `RATE_LIMIT_BURST` | `60` | Burst limit |
| `RATE_LIMIT_IMAGE_PER_MINUTE` | `20` | Limit decode gambar |
| `MAX_JSON_BYTES` | `262144` | 256 KB |
| `MAX_IMAGE_BYTES` | `2097152` | 2 MB |
| `MAX_IMAGE_PIXELS` | `4194304` | 2048×2048 |
| `MAX_PAYLOAD_BYTES` | `4096` | Panjang payload QRIS |
| `DEFAULT_TTL_SECONDS` | `900` | TTL transaksi default |
| `SWEEPER_INTERVAL_SECONDS` | `30` | Interval sweeper |
| `SWEEPER_GRACE_SECONDS` | `300` | Toleransi notifikasi terlambat |
| `WEBHOOK_TIMEOUT_SECONDS` | `10` | Timeout outbound |
| `WEBHOOK_MAX_ATTEMPTS` | `6` | Jumlah percobaan |
| `CORS_ALLOWED_ORIGINS` | kosong | Pisahkan dengan koma; kosong = deny |
| `TRUST_PROXY` | `false` | Percayai `X-Forwarded-For` hanya di belakang proxy |
| `ALLOW_INSECURE_CALLBACKS` | `false` | Hanya untuk dev; jangan di produksi |
| `RUST_LOG` | `info` | Filter tracing |

---

## 7. API Overview

Base path: `/api/v1`. Detail kontrak: [ERP.md](./ERP.md) §8–§9.

| Method | Path | Akses |
|---|---|---|
| `GET` | `/healthz` | publik |
| `GET` | `/readyz` | publik |
| `POST` | `/qris/parse` | merchant/master |
| `POST` | `/qris/dynamic` | merchant/master |
| `POST` | `/merchants` | master |
| `GET` | `/merchants` | master |
| `GET` | `/merchants/{id}` | master/owner |
| `PATCH` | `/merchants/{id}` | master |
| `DELETE` | `/merchants/{id}` | master |
| `POST` | `/merchants/{id}/keys` | master |
| `POST` | `/merchants/{id}/sources` | master |
| `GET` | `/merchants/{id}/sources` | master |
| `DELETE` | `/sources/{id}` | master |
| `POST` | `/merchants/{id}/transactions` | merchant/master |
| `GET` | `/transactions` | merchant/master |
| `GET` | `/transactions/{id}` | merchant/master |
| `GET` | `/transactions/{id}/status` | merchant/master |
| `GET` | `/transactions/{id}/qr.png` | merchant/master |
| `GET` | `/transactions/{id}/qr.svg` | merchant/master |
| `GET` | `/transactions/{id}/stream` | merchant/master |
| `POST` | `/transactions/{id}/cancel` | merchant/master |
| `POST` | `/transactions/{id}/mark-paid` | master |
| `POST` | `/webhooks/mutation/{source_id}` | HMAC source |
| `POST` | `/merchants/{id}/test-webhook` | master |

> Axum 0.8 memakai `{id}`, bukan `:id`.

---

## 8. Contoh Penggunaan

> **Autentikasi contoh.** `Authorization: Bearer` adalah identitas pemanggil.
> Semua contoh POST di bawah memakai Bearer saja dan mengasumsikan
> `REQUIRE_SIGNED_REQUESTS=false` **hanya untuk lokal**. Di produksi, setiap request
> non-GET juga wajib membawa `X-Qris-Timestamp`, `X-Qris-Nonce`, dan
> `X-Qris-Signature`; signature **tidak menggantikan** Bearer. Kanonik dan aturan
> lengkap ada di [ERP.md](./ERP.md) §8.3. Inbound webhook (§8.6) adalah pengecualian:
> selalu memakai HMAC source secret, bukan Bearer.

### 8.1 Parse QRIS

```bash
export BASE=http://127.0.0.1:8080
export API_KEY=qsk_live_...
export STATIC_PAYLOAD='00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB'

curl -sS -X POST "$BASE/api/v1/qris/parse" \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"payload\":\"$STATIC_PAYLOAD\",\"include_tlv\":true}"
```

Atau upload gambar:

```bash
curl -sS -X POST "$BASE/api/v1/qris/parse" \
  -H "Authorization: Bearer $API_KEY" \
  -F "image=@qris.jpeg"
```

### 8.2 Konversi stateless

```bash
curl -sS -X POST "$BASE/api/v1/qris/dynamic" \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"payload\":\"$STATIC_PAYLOAD\",
    \"amount\":500000,
    \"fee\":{\"mode\":\"NONE\"},
    \"unique_code\":{\"enabled\":false},
    \"output\":{\"png\":true,\"svg\":false}
  }"
```

Output berisi `payload`, `crc`, rincian `amount`, `merchant`, dan `qr.png_base64`.
Tidak ada data yang disimpan.

### 8.3 Daftarkan merchant (master)

```bash
export MASTER_KEY=...
curl -sS -X POST "$BASE/api/v1/merchants" \
  -H "Authorization: Bearer $MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"name\":\"Kos Sayu\",
    \"static_payload\":\"$STATIC_PAYLOAD\",
    \"default_unique_code\":true,
    \"callback_url\":\"https://app.example.com/webhooks/qris\"
  }"
```

Simpan `credentials.api_key` dan `credentials.signing_secret`; keduanya hanya tampil sekali.

### 8.4 Buat transaksi

```bash
export MERCHANT_ID=mch_...
export MERCHANT_KEY=qsk_live_...
curl -sS -X POST "$BASE/api/v1/merchants/$MERCHANT_ID/transactions" \
  -H "Authorization: Bearer $MERCHANT_KEY" \
  -H "Idempotency-Key: order-123" \
  -H "Content-Type: application/json" \
  -d '{
    "base_amount":500000,
    "reference_label":"INV-2026-0001",
    "unique_code":{"enabled":true},
    "ttl_seconds":900
  }'
```

API mengalokasikan kode unik (mis. `142`) dan mengembalikan nominal final `500142`,
payload dinamis, PNG, dan `expires_at`.

### 8.5 Cek status & SSE

```bash
export TXN_ID=txn_...
curl -sS "$BASE/api/v1/transactions/$TXN_ID/status" \
  -H "Authorization: Bearer $MERCHANT_KEY"

curl -N "$BASE/api/v1/transactions/$TXN_ID/stream" \
  -H "Authorization: Bearer $MERCHANT_KEY"
```

SSE mengirim:

```
event: status
data: {"id":"txn_...","status":"PAID","paid_at":"2026-11-01T10:04:31Z","amount":{"final":500142}}
```

### 8.6 Kirim notifikasi mutasi (HMAC)

```python
import hashlib, hmac, json, time, requests

secret = "qws_live_..."          # source secret, tampil sekali saat dibuat
event_id = "notif-12345"
body = json.dumps({
    "package": "com.gojek.merchant",
    "app_name": "GoPay Merchant",
    "title": "Pembayaran diterima",
    "text": "Pembayaran QRIS Rp 500.142 berhasil diterima.",
    "posted_at": "2026-11-01T10:04:30Z",
    "notification_id": "0|com.gojek.merchant|12345",
}, separators=(",", ":"))

ts = str(int(time.time()))
canonical = f"{ts}\n{event_id}\n{hashlib.sha256(body.encode()).hexdigest()}"
signature = "v1=" + hmac.new(secret.encode(), canonical.encode(), hashlib.sha256).hexdigest()

requests.post(
    "https://qris.example.com/api/v1/webhooks/mutation/src_...",
    data=body,
    headers={
        "Content-Type": "application/json",
        "X-Qris-Timestamp": ts,
        "X-Qris-Event-Id": event_id,
        "X-Qris-Signature": signature,
    },
    timeout=10,
)
```

Response `202`:

```json
{"event_id":"evt_...","status":"MATCHED","transaction_id":"txn_...","reason":"exact_amount_match"}
```

Status event: `MATCHED` | `UNMATCHED` | `DUPLICATE` | `IGNORED` | `ERROR`.

---

## 9. Auto-Detect Pembayaran

| Kanal | Sumber | Contoh |
|---|---|---|
| **Android** | Aplikasi forwarder di HP merchant (MacroDroid/Tasker/aplikasi sendiri) | Notifikasi GoPay Merchant, DANA Bisnis, BCA, BRI, Livin, ShopeePay |
| **Email** | Cloudflare Email Worker / IMAP forwarder | Email mutasi bank diteruskan sebagai JSON |
| **Aggregator** | Moota / CekMutasi / webhook kustom | Nominal terstruktur + referensi |

Alur:

1. Notifikasi masuk ke `POST /api/v1/webhooks/mutation/{source_id}` dengan HMAC.
2. Parser mengekstrak nominal + arah (masuk/keluar).
3. Matcher mencari transaksi `PENDING` dengan `final_amount` **persis** sama.
4. Jika satu kandidat → `PAID`; jika nol → `UNMATCHED` (disimpan).
5. SSE dikirim; outbound webhook `transaction.paid` dipanggil dengan retry.

Yang **tidak** dilakukan:

- Tidak login ke akun bank.
- Tidak scraping aplikasi merchant.
- Tidak melakukan fuzzy match berdasarkan nama atau deskripsi.
- Tidak menandai lunas hanya karena nominal dasar cocok.

---

## 10. Keamanan

- **Master key** untuk admin; **merchant key** (`qsk_live_...`) hanya hash SHA-256
  yang disimpan; **signing/source secret** dienkripsi AES-256-GCM.
- **Signed request** HMAC-SHA256: timestamp + nonce + method + path + hash body.
  Nonce disimpan di DB untuk mencegah replay.
- **Rate limit** per IP dan per kredensial; upload gambar lebih ketat.
- **Upload**: magic bytes, 2 MB, 2048×2048, decode di `spawn_blocking`.
- **SSRF**: callback wajib HTTPS publik, tanpa redirect.
- **Log**: tidak memuat key, secret, signature, payload QRIS, atau teks notifikasi mentah.
- **Error**: envelope terstruktur; 5xx tidak membocorkan stack trace/SQL.
- **CORS**: allowlist; default deny.
- **Audit**: aksi admin, mark-paid, rotasi key, dan perubahan status.

Checklist produksi:

- [ ] TLS di reverse proxy.
- [ ] `APP_BIND=127.0.0.1` (proxy yang expose).
- [ ] `REQUIRE_SIGNED_REQUESTS=true`.
- [ ] `ALLOW_PUBLIC_STATELESS=false`.
- [ ] `MASTER_API_KEY` dan `DATA_ENCRYPTION_KEY` dari secret manager.
- [ ] Backup harian + `PRAGMA integrity_check`.
- [ ] Monitoring 5xx, `UNMATCHED`, `DEAD` delivery, auth failure.
- [ ] Rotasi key/secret terjadwal.

---

## 11. Deployment

### 11.1 Docker (target M4)

```dockerfile
FROM rust:1.85-slim AS build
WORKDIR /app
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd -r -u 10001 qris && mkdir /data && chown qris /data
COPY --from=build /app/target/release/qris-api /usr/local/bin/qris-api
USER qris
ENV APP_BIND=0.0.0.0:8080 DATABASE_URL=sqlite:///data/qris.db
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["qris-api"]
```

```bash
docker build -t qris-static2dynamic .
docker run -d --name qris \
  -p 127.0.0.1:8080:8080 \
  -v qris-data:/data \
  --env-file .env \
  -e APP_ENV=production \
  -e APP_BIND=0.0.0.0:8080 \
  -e DATABASE_URL=sqlite:///data/qris.db \
  -e REQUIRE_SIGNED_REQUESTS=true \
  qris-static2dynamic
```

> `--env-file .env` dapat menimpa `APP_BIND`, `DATABASE_URL`, dan
> `REQUIRE_SIGNED_REQUESTS` dari image. Opsi `-e` di atas memaksa container
> mendengarkan di `0.0.0.0:8080`, memakai volume `/data`, dan tetap mewajibkan
> signature. Pastikan `.env` produksi tidak berisi `REQUIRE_SIGNED_REQUESTS=false`.

### 11.2 Reverse proxy (Caddy)

```caddyfile
qris.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

Jangan expose port API langsung ke internet. Aktifkan firewall dan batasi akses admin.

---

## 12. Operasi

| Kebutuhan | Cara |
|---|---|
| Health | `GET /healthz` (liveness), `GET /readyz` (DB) |
| Migrasi | `sqlx migrate run` atau `qris-cli migrate` |
| Backup | `qris-cli backup --out /backup/qris-YYYYMMDD.db` (`VACUUM INTO`) |
| Restore | Hentikan API → ganti DB → `integrity_check` → migrate → start |
| Rotasi master key | Set `MASTER_API_KEY_PREVIOUS` → deploy → update client → hapus lama |
| Rotasi merchant key | `POST /merchants/{id}/keys` (key & secret baru tampil sekali) |
| Expiry | Sweeper otomatis setiap 30 detik |
| Retensi | Worker harian: event 90 hari, transaksi/audit 365 hari |

---

## 13. Testing

| Level | Isi |
|---|---|
| Unit | CRC16, TLV, transformasi, fee, parser, masking |
| Property | invariant merchant-safe, idempoten, payload acak |
| Integration | auth, merchant, transaksi, idempotency, webhook flow, SSE |
| Security | signature/replay, rate limit, body limit, SSRF, redaksi log |
| Fixture | decode dua QRIS asli (lokal, `#[ignore]`) + golden vector sintetis |

Golden vector ada di [RULES.md](./RULES.md) §15. Fixture asli:

- `Qris Static.jpeg`
- `Qris Static 2.jpeg`

Jangan commit payload/NMID/nama merchant asli ke test publik. Test fixture asli harus
membaca `QRIS_REAL_FIXTURE_DIR` dan hanya berjalan lokal.

---

## 14. Troubleshooting

| Gejala | Penyebab umum | Solusi |
|---|---|---|
| `QRIS_INVALID_CRC` | Payload terpotong/salah salin | Ambil ulang dari gambar; jangan edit manual |
| `QRIS_UNSUPPORTED_FEE_TAGS` | Payload memuat tag `55/56/57` | Gunakan QRIS statis polos; fee dihitung API |
| `QRIS_QR_NOT_FOUND` | Gambar blur/terlalu kecil | Crop area QR, resolusi lebih tinggi |
| `QRIS_IMAGE_DIMENSIONS_EXCEEDED` | Gambar > 2048×2048 | Perkecil sebelum upload |
| `SIGNATURE_INVALID` | Kanonik/body/timestamp salah | Pastikan body byte-identik dan path+query persis |
| `SIGNATURE_REPLAYED` | Nonce dipakai ulang | Generate nonce baru per request |
| `RATE_LIMITED` | Terlalu banyak request | Hormati `Retry-After`; naikkan limit jika perlu |
| `UNMATCHED` | Nominal tidak sama / di luar TTL | Cek kode unik, TTL, dan teks notifikasi |
| `DEAD` delivery | Callback mati/4xx permanen | Perbaiki endpoint; replay manual (v1.1) |
| `database is locked` | Transaksi panjang/instance ganda | WAL + transaksi pendek; v1 single instance |

---

## 15. Dokumentasi

| Dokumen | Isi |
|---|---|
| [PRD.md](./PRD.md) | Latar belakang, tujuan, epic, NFR, risiko, roadmap |
| [ERP.md](./ERP.md) | ERD, DDL, state machine, kontrak JSON, signature |
| [RULES.md](./RULES.md) | Aturan normatif, kode error, golden vector |
| [AGENTS.md](./AGENTS.md) | Panduan agent, konvensi Rust, DoD, larangan |

---

## 16. Disclaimer

- Proyek ini **bukan** payment gateway, bukan penyelenggara jasa pembayaran, dan tidak
  menyimpan dana.
- Dana tetap masuk ke rekening QRIS merchant. Merchant bertanggung jawab atas kepatuhan
  terhadap acquirer/PTEN, ASPI, dan regulasi Bank Indonesia.
- Perubahan payload hanya pada `01/54/63`; tetap lakukan pilot per acquirer sebelum
  produksi.
- Auto-detect bergantung pada notifikasi yang diteruskan merchant; tidak ada jaminan
  pembayaran terdeteksi jika notifikasi tidak pernah sampai.

---

## 17. Lisensi

Belum ditentukan. Tambahkan file `LICENSE` sebelum publikasi.
