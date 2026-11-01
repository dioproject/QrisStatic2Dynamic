# AGENTS.md — Panduan AI Coding Agent — QrisStatic2Dynamic

> Versi: 1.0 · Status: Final · Tanggal: 2026-11-01
> Dokumen ini mengikat semua AI agent yang bekerja di repositori ini.
> Dokumen terkait: [PRD](./PRD.md) · [ERP](./ERP.md) · [RULES](./RULES.md) · [README](./README.md)

---

## 1. Tujuan Repositori

Membangun REST API Rust + Axum untuk:

1. Validasi payload QRIS statis (EMVCo TLV + CRC16).
2. Konversi QRIS statis → dinamis secara **merchant-safe**.
3. Decode QR dari gambar dan render QR ke PNG/SVG/base64.
4. Mode stateless dan stateful (merchant, transaksi, kode unik).
5. Auto-detect pembayaran dari tiga kanal: Android notification, email, aggregator.
6. Menjaga keamanan, auditabilitas, dan konsistensi data.

---

## 2. Source of Truth

Baca dengan urutan ini sebelum mengubah kode:

1. **`RULES.md`** — aturan normatif; jika kode melanggar, kode yang salah.
2. **`PRD.md`** — tujuan, scope, non-goal, acceptance criteria.
3. **`ERP.md`** — ERD/DDL, kontrak JSON, signature, state machine.
4. **`README.md`** — cara menjalankan dan mengoperasikan.
5. **`AGENTS.md`** — cara bekerja (dokumen ini).

Jika permintaan pengguna bertentangan dengan `RULES.md`:

- Jangan langsung mengubah aturan.
- Jelaskan konfliknya, tunjukkan rule ID yang terdampak, dan minta keputusan.
- Perubahan aturan harus disertai update dokumen + test.

---

## 3. Invariant yang Tidak Boleh Dilanggar

Ringkasan dari `RULES.md` §1:

1. Hanya tag `01`, `54`, `63` yang boleh berubah.
2. Tag `26–51`, `52`, `53`, `58`, `59`, `60`, `61`, `62`, `64` wajib byte-identik.
3. Tag `55/56/57` dilarang masuk input dan dilarang dibuat.
4. `reference_label` / nomor invoice tidak boleh masuk payload.
5. Uang selalu integer rupiah; tidak ada `f64`.
6. Auto-match hanya exact `final_amount`, status `PENDING`, dalam TTL + grace.
7. API key di-hash; signing/source secret dienkripsi; tidak pernah di-log.
8. Signature + nonce wajib untuk request non-GET saat strict mode.
9. Upload gambar wajib dibatasi byte, piksel, format, dan di-decode di `spawn_blocking`.
10. Callback URL wajib HTTPS publik; tolak SSRF.

Jika ragu, cari rule ID terkait dan tambahkan test yang membuktikannya.

---

## 4. Status & Struktur Target

Saat dokumen ini dibuat, repositori masih berisi spesifikasi dan dua fixture QRIS.
Implementasi Rust belum ada. Struktur target:

```
QrisStatic2Dynamic/
├── Cargo.toml
├── Cargo.lock
├── .env.example
├── .gitignore
├── Dockerfile
├── README.md
├── PRD.md
├── ERP.md
├── RULES.md
├── AGENTS.md
├── Qris Static.jpeg          # fixture lokal, jangan commit ke repo publik
├── Qris Static 2.jpeg        # fixture lokal, jangan commit ke repo publik
├── migrations/
│   └── 0001_init.sql
├── src/
│   ├── main.rs               # bootstrap, config, tracing, graceful shutdown
│   ├── lib.rs                # router + state untuk integration test
│   ├── config.rs             # env parsing, validasi startup
│   ├── state.rs              # AppState (pool, config, broadcast)
│   ├── error.rs              # AppError + IntoResponse + kode error
│   ├── telemetry.rs          # tracing JSON, request id
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── amount.rs         # Rupiah newtype, fee, validasi
│   │   ├── crc16.rs          # CRC-16/CCITT-FALSE
│   │   ├── tlv.rs            # parse/encode TLV + nested
│   │   ├── qris.rs           # validate, extract, transform, invariant
│   │   ├── image.rs          # decode QR dari gambar
│   │   └── render.rs         # PNG/SVG/base64
│   ├── auth/
│   │   ├── mod.rs            # extractor Principal
│   │   ├── master.rs         # constant-time master key
│   │   ├── api_key.rs        # prefix lookup + SHA-256
│   │   ├── hmac.rs           # canonical + verify + nonce
│   │   └── crypto.rs         # AES-256-GCM untuk secret
│   ├── db/
│   │   ├── mod.rs            # pool, pragmas, migrate
│   │   └── repos/
│   │       ├── merchants.rs
│   │       ├── keys.rs
│   │       ├── sources.rs
│   │       ├── transactions.rs
│   │       ├── events.rs
│   │       ├── deliveries.rs
│   │       └── audit.rs
│   ├── services/
│   │   ├── qris_service.rs   # stateless parse/dynamic
│   │   ├── merchant_service.rs
│   │   ├── transaction_service.rs
│   │   ├── parser.rs         # notifikasi → nominal/direction
│   │   ├── matcher.rs        # exact match + transaksi atomik
│   │   ├── outbound.rs       # worker outbox + HMAC + retry
│   │   ├── sweeper.rs        # expiry + nonce + retensi
│   │   └── sse.rs            # broadcast status
│   ├── api/
│   │   ├── mod.rs            # router /api/v1
│   │   ├── middleware/
│   │   │   ├── request_id.rs
│   │   │   ├── rate_limit.rs
│   │   │   ├── security_headers.rs
│   │   │   └── body_limit.rs
│   │   └── handlers/
│   │       ├── health.rs
│   │       ├── qris.rs
│   │       ├── merchants.rs
│   │       ├── transactions.rs
│   │       ├── webhooks.rs
│   │       └── stream.rs
│   └── bin/
│       └── qris-cli.rs       # migrate, backup, rotate, cleanup
├── tests/
│   ├── qris_vectors.rs
│   ├── auth.rs
│   ├── transactions.rs
│   ├── webhook_flow.rs
│   └── fixtures/             # hanya fixture sintetis untuk CI
└── .github/workflows/ci.yml
```

Nama modul boleh disesuaikan jika ada alasan kuat, tetapi **batas tanggung jawab**
harus tetap: domain murni, service, repo, handler, auth, worker.

---

## 5. Toolchain & Perintah

Target: **Rust stable ≥ 1.85, edition 2024** (sesuaikan dengan toolchain yang tersedia).

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo run
RUST_LOG=info,qris_api=debug cargo run
```

Database:

```bash
export DATABASE_URL="sqlite://./data/qris.db"
sqlx database create
sqlx migrate run
```

Atau jalankan migrasi otomatis saat startup (`sqlx::migrate!`) dan sediakan
`qris-cli migrate` untuk operasi manual.

Quality gate tambahan (CI):

```bash
cargo audit
cargo deny check
```

Jangan menambahkan dependency tanpa:

- alasan yang jelas di PR,
- lisensi kompatibel,
- fitur minimal (matikan default features bila tidak perlu),
- tidak ada C/C++ build yang tidak perlu.

---

## 6. Konvensi Rust

### 6.1 Error handling

- Gunakan `thiserror` untuk error domain dan `AppError` untuk boundary HTTP.
- `AppError` wajib punya `code: &'static str`, `status: StatusCode`, `message`, `details`.
- Implementasikan `IntoResponse` satu kali; jangan mengulang JSON error di handler.
- **Dilarang** `.unwrap()` / `.expect()` / `panic!` di jalur request.
  Boleh di test, startup config yang sudah divalidasi, atau invariant yang tidak mungkin gagal
  (tetap beri komentar).
- Jangan mengembalikan `anyhow::Error` ke client. Map ke `INTERNAL_ERROR` dan log internal.
- Error 5xx tidak boleh memuat detail internal.

### 6.2 Tipe domain

- `struct Rupiah(i64)` untuk uang; implementasi `Add`, `CheckedAdd`, `Display` tanpa desimal.
- `struct TimestampMs(i64)` untuk waktu DB; konversi ke RFC3339 hanya di serializer API.
- ID memakai newtype (`MerchantId`, `TransactionId`, `SourceId`) + prefix ULID.
- Status memakai `enum` dengan `sqlx::Type` dan `serde::Serialize/Deserialize`.
- Jangan memakai `String` mentah untuk uang/status di service.

### 6.3 Async & CPU

- Decode gambar, render QR, dan hashing berat wajib `tokio::task::spawn_blocking`.
- Jangan menahan `Mutex`/transaksi DB melewati `.await` yang tidak perlu.
- Gunakan `tokio::time::timeout` untuk operasi eksternal.
- Worker berjalan sebagai task terpisah dengan graceful shutdown (`CancellationToken`).

### 6.4 Database

- Semua query memakai bind parameter; tidak ada format string SQL.
- Gunakan `sqlx::query_as` + `FromRow`; hindari `SELECT *`.
- Transaksi tulis yang melibatkan alokasi kode unik memakai `BEGIN IMMEDIATE`.
- Jangan menyimpan secret plaintext.
- Jangan hard-delete transaksi/audit/merchant (soft delete).
- Migrasi additive; file bernomor dan tidak boleh diubah setelah dirilis.

### 6.5 API

- Axum 0.8: path param `{id}`, bukan `:id`.
- Router versioned `/api/v1`.
- Response sukses = resource langsung; list = `{items, next_cursor}`.
- Error = envelope `{"error":{...}}` (ERP §8.4).
- Gunakan status code yang benar: 201 create, 202 webhook, 204 delete, 409 conflict, 422 validasi.
- Jangan mengembalikan `200` untuk error.
- Setiap handler menerima `RequestId` dan menuliskannya ke response header.
- Auth diekstrak lewat extractor, bukan dicek manual di tiap handler.
- Scope check merchant key dilakukan di service/repo, bukan hanya di UI.

### 6.6 Logging

- `tracing` JSON di produksi; pretty di dev.
- Field wajib: `request_id`, `method`, `path`, `status`, `latency_ms`, `principal_id`.
- Dilarang log: `Authorization`, API key, secret, signature, nonce, payload QRIS,
  teks notifikasi mentah, body request penuh.
- Gunakan `#[instrument(skip_all, fields(...))]` untuk handler sensitif.
- Log error internal dengan `error!`, response client tetap generik.

### 6.7 Serde

- `#[serde(rename_all = "snake_case")]` untuk API.
- Tolak field tak dikenal pada request penting (`#[serde(deny_unknown_fields)]`)
  agar typo tidak diabaikan diam-diam.
- Nominal sebagai `i64`; jangan `f64`.
- Waktu sebagai string RFC3339 di API, `i64` di internal.

---

## 7. Keamanan — Checklist Wajib

- [ ] Master key dan `DATA_ENCRYPTION_KEY` dibaca dari env; tidak ada default di kode.
- [ ] API key di-hash SHA-256; prefix lookup; perbandingan constant-time.
- [ ] Signing/source secret dienkripsi AES-256-GCM; AAD mengikat record.
- [ ] Signature HMAC + timestamp + nonce diverifikasi sebelum body diproses.
- [ ] Nonce disimpan di DB; replay ditolak.
- [ ] Rate limit per IP dan per kredensial; 429 + `Retry-After`.
- [ ] Body limit JSON/multipart; image magic bytes + dimensi + timeout.
- [ ] Decode gambar di `spawn_blocking`; tidak ada alokasi tak terbatas.
- [ ] CORS allowlist; security headers; `Cache-Control: no-store` untuk transaksi.
- [ ] Callback URL HTTPS publik; tolak IP privat/loopback/link-local; no redirect.
- [ ] Log redaction diuji.
- [ ] Error 5xx tidak membocorkan stack trace/SQL.
- [ ] Dependency audit (`cargo audit`/`cargo deny`).

Perubahan yang melemahkan salah satu item di atas **ditolak** kecuali ada ADR
dan persetujuan eksplisit pemilik proyek.

---

## 8. Testing

### 8.1 Wajib ada

| Area | Test |
|---|---|
| CRC | `123456789` → `29B1` |
| TLV | parse/encode, nested, duplicate, truncation |
| Transform | golden vector `RULES.md` §15 |
| Invariant | semua tag non-mutasi identik + urutan sama |
| Fee | NONE/FIXED/PERCENT, floor, batas |
| Kode unik | unik, reuse setelah terminal, pool habis, manual conflict |
| Auth | master, merchant, revoked, expired, scope |
| Signature | valid, salah, skew, replay, nonce beda body |
| Webhook | Android/Email/Aggregator, duplicate, unmatched, direction |
| Matcher | exact match, no match, race, expired, tag 62 preservation |
| Upload | magic bytes, >2 MB, >2048px, image bomb, no QR |
| SSRF | callback ke localhost/private IP ditolak |
| Redaction | tidak ada secret/payload di log |
| DB | migrasi, constraint, idempotency, partial index |

### 8.2 Gaya test

- Unit test di modul `#[cfg(test)]`.
- Property test (`proptest`) untuk transformasi acak:
  - output selalu valid;
  - tag non-mutasi identik;
  - idempoten.
- Integration test memakai `Router` + `tower::ServiceExt::oneshot` dan SQLite sementara
  (`tempfile`).
- Jangan memanggil jaringan nyata. Untuk outbound webhook, gunakan server lokal
  atau trait yang bisa di-mock.
- Test yang lambat/bergantung fixture asli diberi `#[ignore]` atau env gate.

### 8.3 Fixture QRIS asli

Dua file di root:

- `Qris Static.jpeg`
- `Qris Static 2.jpeg`

Aturan:

1. **Jangan** menyalin payload asli, NMID, nama merchant, atau kota ke test yang di-commit,
   dokumentasi publik, log, atau issue.
2. Test fixture asli harus membaca path dari env `QRIS_REAL_FIXTURE_DIR` dan ditandai
   `#[ignore]` secara default.
3. Untuk CI publik, gunakan fixture sintetis di `tests/fixtures/`.
4. Ekspektasi lokal (tanpa data merchant):
   - Fixture 1: CRC statis `B676`; dynamic 500142 CRC `A1E6`.
   - Fixture 2: CRC statis `F145`; dynamic 500142 CRC `B20F`.
   - Fixture 2 memiliki tag `62` = `0703A01`; wajib tetap utuh.
   - NMID harus diambil dari akun `ID.CO.QRIS` (tag `51`), bukan ID internal acquirer
     di tag `26`.
5. Jangan commit kedua JPEG ke repositori publik tanpa izin pemilik.

### 8.4 Definition of Done test

- Semua test lama lulus.
- Test baru menutup rule ID yang berubah.
- `cargo clippy -- -D warnings` bersih.
- Tidak ada test yang di-skip tanpa alasan.

---

## 9. Alur Kerja Agent

1. **Baca** `RULES.md` bagian yang relevan + `PRD.md` + `ERP.md`.
2. **Ringkas** requirement dan sebutkan rule ID yang terdampak.
3. **Cek** apakah perubahan menyentuh invariant keamanan/merchant-safe.
4. **Tulis test gagal** (TDD) untuk perilaku baru.
5. **Implementasi minimal**; jangan refactor besar sekaligus.
6. **Jalankan** fmt, clippy, test.
7. **Audit keamanan** dengan checklist §7.
8. **Update dokumen** jika perilaku/kontrak berubah.
9. **Laporkan** ringkasan, file, test, risiko, dan follow-up.

Jika ada ambiguitas yang mengubah kontrak API atau aturan merchant-safe, **bertanya**
sebelum menulis kode.

---

## 10. Konvensi Commit & PR

Commit:

```
feat(core): implement TLV parser and CRC16
fix(auth): reject replayed nonce before body parsing
test(qris): add golden vectors for two-account payload
docs(rules): clarify tag 62 immutability
```

PR wajib memuat:

- ringkasan perubahan,
- rule ID yang terdampak,
- cara test,
- risiko/rollback,
- checklist keamanan,
- screenshot/log hanya jika tidak memuat data sensitif.

Jangan:

- force push ke branch bersama,
- menghapus/melemahkan test,
- menonaktifkan CI,
- commit `.env`, database, backup, atau fixture asli.

---

## 11. Definition of Done (DoD)

Sebuah task selesai jika:

- [ ] Perilaku sesuai `PRD.md` dan `RULES.md`.
- [ ] Test unit/integration/property yang relevan lulus.
- [ ] `cargo fmt --check` dan `cargo clippy -D warnings` bersih.
- [ ] Tidak ada secret/payload di log.
- [ ] Error baru terdaftar di `RULES.md` §14.
- [ ] Perubahan skema punya migrasi additive + test.
- [ ] Dokumen (`PRD`/`ERP`/`RULES`/`README`) diperbarui jika perlu.
- [ ] Checklist keamanan §7 terpenuhi.
- [ ] Tidak ada regresi pada golden vector dan invariant merchant-safe.

---

## 12. Larangan Keras

1. Mengubah tag `62` atau menambahkan sub-tag ke dalamnya.
2. Membuat/mengirim tag `55/56/57`.
3. Memakai `f64`/desimal untuk uang.
4. Menyimpan API key/signing secret/source secret plaintext.
5. Mencatat secret, payload, atau teks notifikasi mentah di log.
6. Menonaktifkan auth, signature, rate limit, atau batas upload "sementara".
7. `.unwrap()`/`.expect()` di handler/service yang menangani input pengguna.
8. Query SQL dengan interpolasi string.
9. Mengikuti redirect saat mengirim outbound webhook.
10. Commit fixture QRIS asli atau data merchant ke repo publik.
11. Menghapus transaksi/audit dengan hard delete.
12. Mengubah migrasi yang sudah dirilis.

---

## 13. Handoff Template

Saat menyerahkan pekerjaan, tulis:

```markdown
## Ringkasan
...

## Rule yang terdampak
- RULES §...

## Perubahan
- file ...

## Test
- `cargo test ...` (hasil)
- test baru: ...

## Risiko / rollback
...

## Follow-up
...
```

---

## 14. Milestone Implementasi

| Milestone | Fokus | Bukti selesai |
|---|---|---|
| **M1** | `domain/` (TLV, CRC, transform, amount, image, render) + handler stateless | Golden vector + property test + decode/encode roundtrip |
| **M2** | migrasi, repo, auth, merchant/transaction, unique code, expiry | Integration test CRUD + idempotency + constraint |
| **M3** | webhook 3 kanal, parser, matcher, SSE, outbound worker | E2E notifikasi → PAID → callback |
| **M4** | rate limit, upload hardening, audit, Docker, CI, backup | Security checklist + load test dasar + deploy |

Jangan mengerjakan M3 sebelum M1/M2 stabil; auto-detect bergantung pada transaksi
dan kode unik yang benar.
